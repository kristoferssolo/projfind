//! Recorded project visits, and the frecency they earn.
//!
//! A project's rank combines how often it was visited with how recently, so a
//! project used twice this hour outranks one used ten times last month. Scores
//! are capped in aggregate and aged down when they reach the cap, which keeps
//! the file bounded and lets old favourites fall away.
//!
//! Pinning opts a project out of that drift: a pinned project ranks above every
//! unpinned one whatever its frecency, and aging never drops it.

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
    /// Whether the project is held above every unpinned one.
    pub pinned: bool,
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
    /// Left out of the file unless set, so an unpinned history reads the same
    /// as one written before pinning existed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
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
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.frecency.total_cmp(&left.frecency))
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

    /// Pins a project so it ranks above every unpinned one, recording it first
    /// if history has not seen it.
    ///
    /// # Errors
    ///
    /// Returns an error when the clock is invalid or the history cannot be written.
    pub fn pin(&mut self, path: &Path) -> Result<()> {
        self.set_pinned(path, true)
    }

    /// Unpins a project, leaving its score and its last visit alone.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is not in history, or it cannot be written.
    pub fn unpin(&mut self, path: &Path) -> Result<()> {
        self.set_pinned(path, false)
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

    /// A project only reaches history by being pinned when it has never been
    /// visited, so it starts at the score one visit would have earned.
    fn set_pinned(&mut self, path: &Path, pinned: bool) -> Result<()> {
        match self.position_of(path) {
            Ok(position) => self.projects[position].pinned = pinned,
            Err(error) if !pinned => return Err(error),
            Err(_) => self.projects.push(ProjectUsage {
                pinned: true,
                ..ProjectUsage::new(path, MINIMUM_SCORE, unix_timestamp(SystemTime::now())?)
            }),
        }
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
            Err(_) => self.projects.push(ProjectUsage::new(
                path,
                score,
                unix_timestamp(SystemTime::now())?,
            )),
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
            self.projects.push(ProjectUsage::new(path, 1.0, timestamp));
        }
        self.age();
        Ok(())
    }

    /// Scales every score down once they total more than [`MAX_TOTAL_SCORE`],
    /// dropping the unpinned projects that fall below [`MINIMUM_SCORE`].
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
            .retain(|project| project.pinned || project.score >= MINIMUM_SCORE);
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
    fn new(path: &Path, score: f64, last_accessed: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            score,
            last_accessed,
            pinned: false,
        }
    }

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
            pinned: self.pinned,
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
    use claims::{assert_err, assert_ok};
    use tempfile::TempDir;

    const NOW: Duration = Duration::from_secs(2_000_000);

    fn usage(path: &str, score: f64, age: Duration) -> ProjectUsage {
        ProjectUsage::new(Path::new(path), score, NOW.saturating_sub(age).as_secs())
    }

    fn pinned(path: &str, score: f64, age: Duration) -> ProjectUsage {
        ProjectUsage {
            pinned: true,
            ..usage(path, score, age)
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
    fn pinned_projects_rank_above_more_frecent_ones() {
        let history = history_of(vec![
            usage("/recent", 100.0, Duration::ZERO),
            pinned("/favourite", 1.0, Duration::from_hours(8 * 24)),
        ]);

        assert_eq!(
            ranking(&history),
            ["/favourite", "/recent"].map(PathBuf::from)
        );
    }

    #[test]
    fn aging_keeps_pinned_projects() {
        let mut history = history_of(vec![
            usage("/frequent", MAX_TOTAL_SCORE, Duration::ZERO),
            pinned("/favourite", 1.0, Duration::ZERO),
        ]);

        history.age();

        assert_eq!(history.projects.len(), 2);
        assert!(history.projects[1].score < MINIMUM_SCORE);
    }

    #[test]
    fn pinning_an_unvisited_project_records_it() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let mut history = History::open(temp.path().join("history.toml"))?;

        history.pin(Path::new("/project"))?;

        assert_eq!(history.projects.len(), 1);
        assert!(history.projects[0].pinned);
        assert!((history.projects[0].score - MINIMUM_SCORE).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn pinning_survives_a_round_trip_and_unpinning() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let database = temp.path().join("history.toml");
        let project = Path::new("/projects/favourite");
        let mut history = History::open(&database)?;
        assert_ok!(history.record_at(project, UNIX_EPOCH + NOW));
        history.pin(project)?;

        let mut history = History::open(&database)?;
        assert!(history.projects[0].pinned);
        // The score the visit earned is left alone by both commands.
        assert!((history.projects[0].score - 1.0).abs() < f64::EPSILON);

        history.unpin(project)?;

        assert!(!History::open(&database)?.projects[0].pinned);
        Ok(())
    }

    #[test]
    fn unpinning_an_unknown_project_fails() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let mut history = History::open(temp.path().join("history.toml"))?;

        assert_err!(history.unpin(Path::new("/project")));
        Ok(())
    }

    #[test]
    fn pruning_keeps_only_projects_that_still_exist() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let present = temp.path().join("present");
        std::fs::create_dir(&present).expect("create project dir");
        let mut history = History::open(temp.path().join("history.toml"))?;
        history
            .projects
            .push(ProjectUsage::new(&present, 1.0, NOW.as_secs()));
        history
            .projects
            .push(usage("/definitely/not/here", 1.0, Duration::ZERO));

        history.prune()?;

        assert_eq!(history.projects.len(), 1);
        assert_eq!(history.projects[0].path, present);
        Ok(())
    }
}
