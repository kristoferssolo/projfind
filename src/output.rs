//! Ranking discovered projects against history, and printing them.

use crate::{
    config::OutputFormat, error::Result, finder::Project, history::HistoryEntry,
    paths::contract_tilde,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// A discovered project, joined to whatever history knows about it.
#[derive(Debug, Serialize)]
pub struct ProjectResult {
    pub path: PathBuf,
    pub score: f64,
    pub frecency: f64,
    pub last_used: Option<u64>,
    pub pinned: bool,
    pub markers: Vec<String>,
}

/// Orders `projects` by pin, then by descending frecency, then by path, and
/// attaches each one's recorded usage.
///
/// Projects history has never seen rank last, in path order.
#[must_use]
pub fn rank_projects(mut projects: Vec<Project>, history: &[HistoryEntry]) -> Vec<ProjectResult> {
    let history_by_path = history
        .iter()
        .map(|entry| (entry.path.as_path(), entry))
        .collect::<HashMap<_, _>>();
    // How a project stands in the ranking: pinned first, then frecency.
    let standing_of = |path: &Path| {
        history_by_path
            .get(path)
            .map_or((false, 0.0), |entry| (entry.pinned, entry.frecency))
    };

    projects.sort_unstable_by(|left, right| {
        let (left_pinned, left_frecency) = standing_of(&left.path);
        let (right_pinned, right_frecency) = standing_of(&right.path);
        right_pinned
            .cmp(&left_pinned)
            .then_with(|| right_frecency.total_cmp(&left_frecency))
            .then_with(|| left.path.cmp(&right.path))
    });

    projects
        .into_iter()
        .map(|project| {
            let entry = history_by_path.get(project.path.as_path());
            ProjectResult {
                path: project.path,
                score: entry.map_or(0.0, |entry| entry.score),
                frecency: entry.map_or(0.0, |entry| entry.frecency),
                last_used: entry.map(|entry| entry.last_used_at),
                pinned: entry.is_some_and(|entry| entry.pinned),
                markers: project.markers,
            }
        })
        .collect()
}

/// Writes project results in the selected command-line format.
///
/// # Errors
///
/// Returns an error if output cannot be written or a JSON record cannot be serialized.
pub fn write_projects(
    writer: &mut impl Write,
    projects: &[ProjectResult],
    format: OutputFormat,
    home: Option<&Path>,
) -> Result<()> {
    for project in projects {
        match format {
            OutputFormat::Path => {
                writeln!(writer, "{}", contract_tilde(&project.path, home).display())?;
            }
            OutputFormat::Json => {
                serde_json::to_writer(&mut *writer, project)?;
                writer.write_all(b"\n")?;
            }
            // Paths are written raw so a consumer sees exactly what to `cd` to.
            OutputFormat::Null => {
                writer.write_all(path_bytes(&project.path))?;
                writer.write_all(b"\0")?;
            }
        }
    }
    Ok(())
}

/// Writes history entries as score, frecency, age, pin, and path columns.
///
/// # Errors
///
/// Returns an error if output cannot be written.
pub fn write_entries(
    writer: &mut impl Write,
    entries: impl IntoIterator<Item = HistoryEntry>,
    home: Option<&Path>,
) -> Result<()> {
    for entry in entries {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}",
            entry.score,
            entry.frecency,
            format_age(entry.last_used),
            if entry.pinned { "pinned" } else { "-" },
            contract_tilde(&entry.path, home).display()
        )?;
    }
    Ok(())
}

/// Renders an age in the largest unit that still leaves a whole number.
fn format_age(age: Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0..MINUTE => format!("{seconds}s"),
        MINUTE..HOUR => format!("{}m", seconds / MINUTE),
        HOUR..DAY => format!("{}h", seconds / HOUR),
        _ => format!("{}d", seconds / DAY),
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(0, "0s")]
    #[case(59, "59s")]
    #[case(60, "1m")]
    #[case(90, "1m")]
    #[case(59 * 60, "59m")]
    #[case(60 * 60, "1h")]
    #[case(23 * 60 * 60, "23h")]
    #[case(24 * 60 * 60, "1d")]
    #[case(400 * 24 * 60 * 60, "400d")]
    fn ages_use_the_largest_whole_unit(#[case] seconds: u64, #[case] expected: &str) {
        assert_eq!(format_age(Duration::from_secs(seconds)), expected);
    }

    #[test]
    fn untracked_projects_rank_below_tracked_ones() {
        let projects = ["/untracked", "/tracked"]
            .map(|path| Project {
                path: PathBuf::from(path),
                markers: vec![".git".to_owned()],
            })
            .to_vec();
        let history = [entry("/tracked", 12.0, false)];

        let ranked = rank_projects(projects, &history);

        assert_eq!(
            paths_of(&ranked),
            [Path::new("/tracked"), Path::new("/untracked")]
        );
    }

    #[test]
    fn pinned_projects_rank_above_more_frecent_ones() {
        let projects = ["/frecent", "/pinned"]
            .map(|path| Project {
                path: PathBuf::from(path),
                markers: vec![".git".to_owned()],
            })
            .to_vec();
        let history = [entry("/frecent", 40.0, false), entry("/pinned", 1.0, true)];

        let ranked = rank_projects(projects, &history);

        assert_eq!(
            paths_of(&ranked),
            [Path::new("/pinned"), Path::new("/frecent")]
        );
        assert!(ranked[0].pinned);
        assert!(!ranked[1].pinned);
    }

    fn entry(path: &str, frecency: f64, pinned: bool) -> HistoryEntry {
        HistoryEntry {
            path: PathBuf::from(path),
            score: 3.0,
            frecency,
            last_used: Duration::ZERO,
            last_used_at: 0,
            pinned,
        }
    }

    fn paths_of(results: &[ProjectResult]) -> Vec<&Path> {
        results.iter().map(|result| result.path.as_path()).collect()
    }
}
