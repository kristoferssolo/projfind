//! Recorded project visits, and the frecency they earn.
//!
//! A project's rank combines how often it was visited with how recently, so a
//! project used twice this hour outranks one used ten times last month. Scores
//! are capped in aggregate and aged down when they reach the cap, which keeps
//! the file bounded and lets old favourites fall away.

use crate::{
    error::{Error, Result},
    fs,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const HISTORY_VERSION: u8 = 1;
const MAX_TOTAL_SCORE: f64 = 10_000.0;
const AGED_TOTAL_SCORE: f64 = MAX_TOTAL_SCORE * 0.9;

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;

/// The lowest score worth keeping. Anything below it is dropped when aging.
const MINIMUM_SCORE: f64 = 1.0;

/// Every project mekle has seen the user visit.
#[derive(Debug)]
pub struct History {
    path: PathBuf,
    projects: Vec<ProjectUsage>,
}

/// One project's standing, as of the moment it was read.
#[derive(Debug)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub score: f64,
    pub frecency: f64,
    /// How long ago the project was last visited.
    pub last_used: Duration,
    /// When the project was last visited, in seconds since the Unix epoch.
    pub last_used_at: u64,
}

/// A requested change to a project's raw score.
#[derive(Debug, Clone, Copy)]
pub enum ScoreChange {
    Set(f64),
    Adjust(f64),
    Remove,
}

/// The history file as it is read back.
#[derive(Debug, Deserialize)]
struct StoredHistory {
    version: u8,
    projects: Vec<ProjectUsage>,
}

/// The history file as it is written, borrowing what is already in memory.
#[derive(Debug, Serialize)]
struct StoredHistoryRef<'a> {
    version: u8,
    projects: &'a [ProjectUsage],
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectUsage {
    path: PathBuf,
    score: f64,
    last_accessed: u64,
}

impl History {
    /// Opens the history at `path`, or creates an empty history if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing history cannot be read or parsed.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let Some(contents) = fs::read(&path)? else {
            return Ok(Self {
                path,
                projects: Vec::new(),
            });
        };

        let stored =
            toml::from_str::<StoredHistory>(&contents).map_err(|source| Error::ParseHistory {
                path: path.clone(),
                source,
            })?;
        if stored.version != HISTORY_VERSION {
            return Err(Error::UnsupportedHistoryVersion {
                path,
                version: stored.version,
            });
        }

        Ok(Self {
            path,
            projects: stored.projects,
        })
    }

    /// Records a project visit and persists the updated history.
    ///
    /// # Errors
    ///
    /// Returns an error when the clock is invalid or the history cannot be written.
    pub fn record(&mut self, path: &Path) -> Result<()> {
        self.record_at(path, SystemTime::now())?;
        self.save()
    }

    /// Returns recorded projects ordered by descending frecency.
    ///
    /// # Errors
    ///
    /// Returns an error if the system clock is before the Unix epoch.
    pub fn entries(&self) -> Result<Vec<HistoryEntry>> {
        Ok(self.entries_at(unix_timestamp(SystemTime::now())?))
    }

    fn entries_at(&self, now: u64) -> Vec<HistoryEntry> {
        let mut entries = self
            .projects
            .iter()
            .map(|project| project.entry(now))
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| {
            right
                .frecency
                .total_cmp(&left.frecency)
                .then_with(|| left.path.cmp(&right.path))
        });
        entries
    }

    /// Applies a score change and persists it.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid scores, missing entries, invalid clocks, or writes.
    pub fn update(&mut self, path: &Path, change: ScoreChange) -> Result<()> {
        match change {
            ScoreChange::Set(score) => self.set_score(path, score)?,
            ScoreChange::Adjust(delta) => self.adjust_score(path, delta)?,
            ScoreChange::Remove => {
                self.projects.remove(self.position_of(path)?);
            }
        }
        self.age();
        self.save()
    }

    /// Removes every entry and persists the empty history.
    ///
    /// # Errors
    ///
    /// Returns an error when the history cannot be written.
    pub fn clear(&mut self) -> Result<()> {
        self.projects.clear();
        self.save()
    }

    /// Removes entries whose project paths no longer exist and persists the history.
    ///
    /// # Errors
    ///
    /// Returns an error when a path cannot be checked or the history cannot be written.
    pub fn prune(&mut self) -> Result<()> {
        // Collected up front so a failed check leaves the history untouched.
        let existing = self
            .projects
            .iter()
            .map(|project| fs::exists(&project.path))
            .collect::<Result<Vec<_>>>()?;

        self.projects = self
            .projects
            .drain(..)
            .zip(existing)
            .filter_map(|(project, exists)| exists.then_some(project))
            .collect();
        self.save()
    }

    fn position_of(&self, path: &Path) -> Result<usize> {
        self.projects
            .iter()
            .position(|project| project.path == path)
            .ok_or_else(|| Error::HistoryEntryNotFound(path.to_path_buf()))
    }

    fn set_score(&mut self, path: &Path, score: f64) -> Result<()> {
        validate_score(score)?;
        match self.position_of(path) {
            Ok(position) => self.projects[position].score = score,
            Err(_) => self.projects.push(ProjectUsage {
                path: path.to_path_buf(),
                score,
                last_accessed: unix_timestamp(SystemTime::now())?,
            }),
        }
        Ok(())
    }

    fn adjust_score(&mut self, path: &Path, delta: f64) -> Result<()> {
        if !delta.is_finite() {
            return Err(Error::InvalidScore(delta));
        }

        let position = self.position_of(path)?;
        let score = self.projects[position].score + delta;
        // Adjusting a project below the floor retires it instead of failing.
        if score < MINIMUM_SCORE {
            self.projects.remove(position);
        } else {
            validate_score(score)?;
            self.projects[position].score = score;
        }
        Ok(())
    }

    fn record_at(&mut self, path: &Path, now: SystemTime) -> Result<()> {
        let timestamp = unix_timestamp(now)?;
        if let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.path == path)
        {
            project.score += 1.0;
            project.last_accessed = timestamp;
        } else {
            self.projects.push(ProjectUsage {
                path: path.to_path_buf(),
                score: 1.0,
                last_accessed: timestamp,
            });
        }
        self.age();
        Ok(())
    }

    /// Scales every score down once they total more than [`MAX_TOTAL_SCORE`],
    /// dropping the projects that fall below [`MINIMUM_SCORE`].
    fn age(&mut self) {
        let total = self
            .projects
            .iter()
            .map(|project| project.score)
            .sum::<f64>();
        if total <= MAX_TOTAL_SCORE {
            return;
        }

        let factor = AGED_TOTAL_SCORE / total;
        for project in &mut self.projects {
            project.score *= factor;
        }
        self.projects
            .retain(|project| project.score >= MINIMUM_SCORE);
    }

    fn save(&self) -> Result<()> {
        let stored = StoredHistoryRef {
            version: HISTORY_VERSION,
            projects: &self.projects,
        };
        let contents =
            toml::to_string_pretty(&stored).map_err(|source| Error::SerializeHistory { source })?;

        fs::write_atomic(&self.path, &contents)
    }
}

impl ProjectUsage {
    /// Weighs a raw score by how recently the project was used.
    fn frecency(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.last_accessed);
        let multiplier = if age < HOUR {
            4.0
        } else if age < DAY {
            2.0
        } else if age < WEEK {
            0.5
        } else {
            0.25
        };
        self.score * multiplier
    }

    fn entry(&self, now: u64) -> HistoryEntry {
        HistoryEntry {
            path: self.path.clone(),
            score: self.score,
            frecency: self.frecency(now),
            last_used: Duration::from_secs(now.saturating_sub(self.last_accessed)),
            last_used_at: self.last_accessed,
        }
    }
}

fn validate_score(score: f64) -> Result<()> {
    if score.is_finite() && score >= MINIMUM_SCORE {
        Ok(())
    } else {
        Err(Error::InvalidScore(score))
    }
}

fn unix_timestamp(time: SystemTime) -> Result<u64> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|source| Error::InvalidSystemTime { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use tempfile::TempDir;

    const NOW: Duration = Duration::from_secs(2_000_000);

    fn usage(path: &str, score: f64, age: Duration) -> ProjectUsage {
        ProjectUsage {
            path: PathBuf::from(path),
            score,
            last_accessed: NOW.saturating_sub(age).as_secs(),
        }
    }

    fn history_of(projects: Vec<ProjectUsage>) -> History {
        History {
            path: PathBuf::new(),
            projects,
        }
    }

    #[test]
    fn missing_file_opens_as_empty_history() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");

        let history = History::open(temp.path().join("missing.toml"))?;

        assert!(history.projects.is_empty());
        Ok(())
    }

    #[test]
    fn records_are_persisted() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let database = temp.path().join("mekle/history.toml");
        let project = Path::new("/projects/favorite");
        let mut history = History::open(&database)?;

        assert_ok!(history.record_at(project, UNIX_EPOCH + NOW));
        history.save()?;
        let history = History::open(database)?;

        assert_eq!(history.projects.len(), 1);
        assert_eq!(history.projects[0].path, project);
        assert!((history.projects[0].score - 1.0).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn repeated_visits_increase_the_score() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let mut history = History::open(temp.path().join("history.toml"))?;

        assert_ok!(history.record_at(Path::new("/project"), UNIX_EPOCH + NOW));
        assert_ok!(history.record_at(Path::new("/project"), UNIX_EPOCH + NOW));

        assert!((history.projects[0].score - 2.0).abs() < f64::EPSILON);
        Ok(())
    }

    /// The paths `history` reports, in the order it reports them.
    fn ranking(history: &History) -> Vec<PathBuf> {
        history
            .entries_at(NOW.as_secs())
            .into_iter()
            .map(|entry| entry.path)
            .collect()
    }

    #[test]
    fn recency_outweighs_a_higher_raw_score() {
        let history = history_of(vec![
            usage("/recent", 2.0, Duration::from_mins(30)),
            usage("/yesterday", 3.0, Duration::from_hours(2)),
            usage("/week", 10.0, Duration::from_hours(2 * 24)),
            usage("/old", 16.0, Duration::from_hours(8 * 24)),
        ]);

        assert_eq!(
            ranking(&history),
            ["/recent", "/yesterday", "/week", "/old"].map(PathBuf::from)
        );
    }

    #[test]
    fn equally_ranked_projects_are_ordered_by_path() {
        let history = history_of(vec![
            usage("/beta", 1.0, Duration::ZERO),
            usage("/alpha", 1.0, Duration::ZERO),
        ]);

        assert_eq!(ranking(&history), ["/alpha", "/beta"].map(PathBuf::from));
    }

    #[test]
    fn aging_removes_projects_with_negligible_scores() {
        let mut history = history_of(vec![
            usage("/frequent", MAX_TOTAL_SCORE, Duration::ZERO),
            usage("/rare", 1.0, Duration::ZERO),
        ]);

        history.age();

        assert_eq!(history.projects.len(), 1);
        assert_eq!(history.projects[0].path, Path::new("/frequent"));
        assert!(history.projects[0].score <= AGED_TOTAL_SCORE);
    }

    #[test]
    fn adjusting_below_the_floor_retires_a_project() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let mut history = History::open(temp.path().join("history.toml"))?;
        history
            .projects
            .push(usage("/project", 2.0, Duration::ZERO));

        history.update(Path::new("/project"), ScoreChange::Adjust(-1.5))?;

        assert!(history.projects.is_empty());
        Ok(())
    }

    #[test]
    fn pruning_keeps_only_projects_that_still_exist() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let present = temp.path().join("present");
        std::fs::create_dir(&present).expect("create project dir");
        let mut history = History::open(temp.path().join("history.toml"))?;
        history.projects.push(ProjectUsage {
            path: present.clone(),
            score: 1.0,
            last_accessed: NOW.as_secs(),
        });
        history
            .projects
            .push(usage("/definitely/not/here", 1.0, Duration::ZERO));

        history.prune()?;

        assert_eq!(history.projects.len(), 1);
        assert_eq!(history.projects[0].path, present);
        Ok(())
    }
}
