use crate::errors::{ProjectFinderError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

const HISTORY_VERSION: u8 = 1;
const MAX_TOTAL_SCORE: f64 = 10_000.0;
const AGED_TOTAL_SCORE: f64 = MAX_TOTAL_SCORE * 0.9;

/// Returns the history file location from the XDG data directory or home directory.
#[must_use]
pub fn history_file_path() -> Option<PathBuf> {
    history_file_path_from(env::var_os("XDG_DATA_HOME"), env::var_os("HOME"))
}

fn history_file_path_from(
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let data_home = xdg_data_home
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|path| path.join(".local/share"))
        })?;

    Some(data_home.join("projfind/history.toml"))
}

#[derive(Debug)]
pub struct History {
    path: PathBuf,
    projects: Vec<ProjectUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredHistory {
    version: u8,
    projects: Vec<ProjectUsage>,
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
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    projects: Vec::new(),
                });
            }
            Err(error) => return Err(ProjectFinderError::read_file(&path, error)),
        };
        let stored = toml::from_str::<StoredHistory>(&contents).map_err(|source| {
            ProjectFinderError::ParseHistory {
                path: path.clone(),
                source,
            }
        })?;
        if stored.version != HISTORY_VERSION {
            return Err(ProjectFinderError::UnsupportedHistoryVersion {
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

    /// Sorts projects by descending frecency, then by path.
    ///
    /// # Errors
    ///
    /// Returns an error if the system clock is before the Unix epoch.
    pub fn sort(&self, projects: &mut [PathBuf]) -> Result<()> {
        self.sort_at(projects, SystemTime::now())
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

    fn sort_at(&self, projects: &mut [PathBuf], now: SystemTime) -> Result<()> {
        let timestamp = unix_timestamp(now)?;
        let frecencies = self
            .projects
            .iter()
            .map(|project| (project.path.as_path(), project.frecency(timestamp)))
            .collect::<HashMap<_, _>>();

        projects.sort_unstable_by(|left, right| {
            let left_score = frecencies.get(left.as_path()).copied().unwrap_or_default();
            let right_score = frecencies.get(right.as_path()).copied().unwrap_or_default();
            right_score
                .total_cmp(&left_score)
                .then_with(|| left.cmp(right))
        });
        Ok(())
    }

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
        self.projects.retain(|project| project.score >= 1.0);
    }

    fn save(&self) -> Result<()> {
        let stored = StoredHistory {
            version: HISTORY_VERSION,
            projects: self
                .projects
                .iter()
                .map(|project| ProjectUsage {
                    path: project.path.clone(),
                    score: project.score,
                    last_accessed: project.last_accessed,
                })
                .collect(),
        };
        let contents = toml::to_string_pretty(&stored)
            .map_err(|source| ProjectFinderError::SerializeHistory { source })?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|source| ProjectFinderError::write_file(parent, source))?;
        let mut temp = NamedTempFile::new_in(parent)
            .map_err(|source| ProjectFinderError::write_file(&self.path, source))?;
        temp.write_all(contents.as_bytes())
            .and_then(|()| temp.as_file_mut().sync_all())
            .map_err(|source| ProjectFinderError::write_file(&self.path, source))?;
        temp.persist(&self.path)
            .map_err(|error| ProjectFinderError::write_file(&self.path, error.error))?;
        sync_parent(parent).map_err(|source| ProjectFinderError::write_file(parent, source))?;
        Ok(())
    }
}

impl ProjectUsage {
    fn frecency(&self, now: u64) -> f64 {
        let age = Duration::from_secs(now.saturating_sub(self.last_accessed));
        let multiplier = if age < Duration::from_hours(1) {
            4.0
        } else if age < Duration::from_hours(24) {
            2.0
        } else if age < Duration::from_hours(7 * 24) {
            0.5
        } else {
            0.25
        };
        self.score * multiplier
    }
}

fn unix_timestamp(time: SystemTime) -> Result<u64> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|source| ProjectFinderError::InvalidSystemTime { source })
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    Ok(())
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

    #[test]
    fn missing_file_opens_as_empty_history() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;

        let history = History::open(temp.path().join("missing.toml"))?;

        assert!(history.projects.is_empty());
        Ok(())
    }

    #[test]
    fn xdg_data_home_takes_precedence() {
        let path = history_file_path_from(Some("/xdg".into()), Some("/home/user".into()));

        assert_eq!(path, Some(PathBuf::from("/xdg/projfind/history.toml")));
    }

    #[test]
    fn home_is_the_fallback_history_location() {
        let path = history_file_path_from(None, Some("/home/user".into()));

        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/user/.local/share/projfind/history.toml"
            ))
        );
    }

    #[test]
    fn records_are_persisted() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let database = temp.path().join("projfind/history.toml");
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
    fn repeated_visits_increase_the_score() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let mut history = History::open(temp.path().join("history.toml"))?;

        assert_ok!(history.record_at(Path::new("/project"), UNIX_EPOCH + NOW));
        assert_ok!(history.record_at(Path::new("/project"), UNIX_EPOCH + NOW));

        assert!((history.projects[0].score - 2.0).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn recency_changes_project_order() -> color_eyre::Result<()> {
        let history = History {
            path: PathBuf::new(),
            projects: vec![
                usage("/recent", 2.0, Duration::from_mins(30)),
                usage("/yesterday", 3.0, Duration::from_hours(2)),
                usage("/week", 10.0, Duration::from_hours(2 * 24)),
                usage("/old", 16.0, Duration::from_hours(8 * 24)),
            ],
        };
        let mut projects = [
            PathBuf::from("/old"),
            PathBuf::from("/week"),
            PathBuf::from("/yesterday"),
            PathBuf::from("/recent"),
        ];

        history.sort_at(&mut projects, UNIX_EPOCH + NOW)?;

        assert_eq!(
            projects,
            ["/recent", "/yesterday", "/week", "/old"].map(PathBuf::from)
        );
        Ok(())
    }

    #[test]
    fn untracked_projects_are_sorted_by_path() -> color_eyre::Result<()> {
        let history = History {
            path: PathBuf::new(),
            projects: Vec::new(),
        };
        let mut projects = [PathBuf::from("/beta"), PathBuf::from("/alpha")];

        history.sort_at(&mut projects, UNIX_EPOCH + NOW)?;

        assert_eq!(projects, ["/alpha", "/beta"].map(PathBuf::from));
        Ok(())
    }

    #[test]
    fn aging_removes_projects_with_negligible_scores() {
        let mut history = History {
            path: PathBuf::new(),
            projects: vec![
                usage("/frequent", MAX_TOTAL_SCORE, Duration::ZERO),
                usage("/rare", 1.0, Duration::ZERO),
            ],
        };

        history.age();

        assert_eq!(history.projects.len(), 1);
        assert_eq!(history.projects[0].path, Path::new("/frequent"));
        assert!(history.projects[0].score <= AGED_TOTAL_SCORE);
    }
}
