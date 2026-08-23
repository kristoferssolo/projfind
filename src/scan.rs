use crate::{
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
};
use regex::escape;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::{debug, warn};

pub struct DirectoryScan {
    pub git_repos: Vec<PathBuf>,
    pub marker_files: HashMap<String, Vec<PathBuf>>,
}

pub async fn scan_directory(
    deps: &Dependencies,
    dir: &Path,
    marker_names: &[String],
    max_depth: usize,
) -> Result<DirectoryScan> {
    let (git_repos, marker_files) = tokio::try_join!(
        find_git_repos(deps, dir, max_depth),
        find_marker_files(deps, dir, marker_names, max_depth),
    )?;

    Ok(DirectoryScan {
        git_repos,
        marker_files,
    })
}

async fn find_marker_files(
    deps: &Dependencies,
    dir: &Path,
    marker_names: &[String],
    max_depth: usize,
) -> Result<HashMap<String, Vec<PathBuf>>> {
    if marker_names.is_empty() {
        return Ok(HashMap::new());
    }

    let combined_patterns = format!(
        "({})",
        marker_names
            .iter()
            .map(|pattern| escape(pattern))
            .collect::<Vec<_>>()
            .join("|")
    );

    let mut cmd = Command::new(&deps.fd_path);
    cmd.arg("--hidden")
        .arg("--no-ignore-vcs")
        .arg("--type")
        .arg("f")
        .arg("--max-depth")
        .arg(max_depth.to_string())
        .arg(&combined_patterns)
        .arg(dir)
        .stdout(Stdio::piped());

    debug!("Finding marker files in {}", dir.display());

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
    let mut results = marker_names
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect::<HashMap<_, _>>();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?
    {
        let path = PathBuf::from(line);
        if let Some(file_name) = path.file_name().and_then(|name| name.to_str())
            && let Some(entries) = results.get_mut(file_name)
        {
            entries.push(path);
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?;
    if !status.success() {
        warn!("fd marker search exited with {status}");
    }

    Ok(results)
}

async fn find_git_repos(deps: &Dependencies, dir: &Path, max_depth: usize) -> Result<Vec<PathBuf>> {
    let mut cmd = Command::new(&deps.fd_path);
    cmd.arg("--hidden")
        .arg("--type")
        .arg("d")
        .arg("--max-depth")
        .arg(max_depth.to_string())
        .arg("^.git$")
        .arg(dir)
        .stdout(Stdio::piped());

    debug!("Finding Git repositories in {}", dir.display());

    let output = cmd
        .output()
        .await
        .map_err(|error| ProjectFinderError::command(&deps.fd_path, error))?;
    if !output.status.success() {
        warn!(
            "fd Git search failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).map_err(|source| ProjectFinderError::Utf8 {
        binary: deps.fd_path.clone(),
        source,
    })?;

    Ok(stdout
        .lines()
        .filter_map(|line| Path::new(line).parent().map(Path::to_path_buf))
        .collect())
}
