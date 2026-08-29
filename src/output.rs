use crate::{
    config::OutputFormat, error::Result, finder::Project, history::HistoryEntry,
    paths::contract_tilde,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct ProjectResult {
    pub path: PathBuf,
    pub score: f64,
    pub frecency: f64,
    pub last_used: Option<u64>,
    pub markers: Vec<String>,
}

#[must_use]
pub fn rank_projects(mut projects: Vec<Project>, history: &[HistoryEntry]) -> Vec<ProjectResult> {
    let history_by_path = history
        .iter()
        .map(|entry| (entry.path.as_path(), entry))
        .collect::<HashMap<_, _>>();

    projects.sort_unstable_by(|left, right| {
        let left_score = history_by_path
            .get(left.path.as_path())
            .map_or(0.0, |entry| entry.frecency);
        let right_score = history_by_path
            .get(right.path.as_path())
            .map_or(0.0, |entry| entry.frecency);
        right_score
            .total_cmp(&left_score)
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
            OutputFormat::Null => {
                writer.write_all(path_bytes(&project.path))?;
                writer.write_all(b"\0")?;
            }
        }
    }
    Ok(())
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
