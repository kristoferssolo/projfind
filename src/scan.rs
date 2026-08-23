use crate::{
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
};
use regex::escape;
use std::{
    iter::once,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::{debug, warn};

const GIT_DIR: &str = ".git";

pub struct DirectoryScan {
    pub git_repos: Vec<PathBuf>,
    pub marker_files: Vec<PathBuf>,
}

/// Scans a directory for repositories and marker files.
///
/// # Errors
///
/// Returns an error if `fd` cannot run or its output cannot be read.
pub async fn scan_directory(
    deps: &Dependencies,
    dir: &Path,
    marker_names: &[String],
    max_depth: usize,
) -> Result<DirectoryScan> {
    let mut cmd = Command::new(&deps.fd_path);
    cmd.arg("--hidden")
        .arg("--follow")
        .arg("--prune")
        .arg("--type")
        .arg("d")
        .arg("--type")
        .arg("f")
        .arg("--max-depth")
        .arg(max_depth.to_string())
        .arg(search_pattern(marker_names))
        .arg(dir)
        .stdout(Stdio::piped());

    debug!("Scanning {}", dir.display());

    let mut child = cmd
        .spawn()
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProjectFinderError::MissingStdout {
            binary: deps.fd_path.clone(),
        })?;
    let mut lines = BufReader::new(stdout).lines();
    let mut scan = DirectoryScan {
        git_repos: Vec::new(),
        marker_files: Vec::new(),
    };

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?
    {
        scan.classify(PathBuf::from(line));
    }

    let status = child
        .wait()
        .await
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?;
    if !status.success() {
        warn!("fd search exited with {status}");
    }

    Ok(scan)
}

fn search_pattern(marker_names: &[String]) -> String {
    let alternatives = once(escape(GIT_DIR))
        .chain(marker_names.iter().map(|name| escape(name)))
        .collect::<Vec<_>>()
        .join("|");

    format!("^({alternatives})$")
}

impl DirectoryScan {
    fn classify(&mut self, path: PathBuf) {
        if path.file_name().is_some_and(|name| name == GIT_DIR) {
            if path.is_dir()
                && let Some(parent) = path.parent()
            {
                self.git_repos.push(parent.to_path_buf());
            }
            return;
        }

        self.marker_files.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_always_looks_for_repositories() {
        assert_eq!(search_pattern(&[]), r"^(\.git)$");
    }

    #[test]
    fn marker_names_are_escaped_into_the_alternation() {
        assert_eq!(
            search_pattern(&["Cargo.toml".to_owned(), "go.mod".to_owned()]),
            r"^(\.git|Cargo\.toml|go\.mod)$"
        );
    }
}
