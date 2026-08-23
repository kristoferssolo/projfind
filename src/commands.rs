use crate::{
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
};
use regex::escape;
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    fs::read_to_string,
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::{debug, warn};

/// Find files matching literal names with `fd`.
pub async fn find_files(
    deps: &Dependencies,
    dir: &Path,
    patterns: &[&str],
    max_depth: usize,
) -> Result<HashMap<String, Vec<PathBuf>>> {
    let combined_patterns = format!(
        "({})",
        patterns
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

    debug!("Running: fd with combined pattern in {}", dir.display());

    let mut child = cmd
        .spawn()
        .map_err(|e| ProjectFinderError::command(&deps.fd_path, e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProjectFinderError::MissingStdout {
            binary: deps.fd_path.clone(),
        })?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut results = patterns
        .iter()
        .map(|pattern| ((*pattern).to_string(), Vec::new()))
        .collect::<HashMap<_, _>>();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| ProjectFinderError::command(&deps.fd_path, e))?
    {
        let path = PathBuf::from(line);
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str())
            && let Some(entries) = results.get_mut(file_name)
        {
            entries.push(path);
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| ProjectFinderError::command(&deps.fd_path, e))?;
    if !status.success() {
        warn!("fd command exited with non-zero status: {status}");
    }

    Ok(results)
}

/// Find Git repository roots with `fd`.
pub async fn find_git_repos(
    deps: &Dependencies,
    dir: &Path,
    max_depth: usize,
) -> Result<Vec<PathBuf>> {
    let mut cmd = Command::new(&deps.fd_path);
    cmd.arg("--hidden")
        .arg("--type")
        .arg("d")
        .arg("--max-depth")
        .arg(max_depth.to_string())
        .arg("^.git$")
        .arg(dir)
        .stdout(Stdio::piped());

    debug!("Finding git repos in {}", dir.display());

    let output = cmd
        .output()
        .await
        .map_err(|e| ProjectFinderError::command(&deps.fd_path, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("fd command failed: {stderr}");
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).map_err(|e| ProjectFinderError::Utf8 {
        binary: deps.fd_path.clone(),
        source: e,
    })?;

    let paths = stdout
        .lines()
        .filter_map(|line| {
            let path = PathBuf::from(line);
            path.parent().map(std::path::Path::to_path_buf)
        })
        .collect();

    Ok(paths)
}

/// A content check for workspace files.
#[derive(Debug, Clone, Copy)]
pub enum ContentTest {
    ContainsAny(&'static [&'static str]),
    LineStartsWith(&'static str),
    NonEmpty,
}

impl ContentTest {
    fn matches(self, contents: &str) -> bool {
        match self {
            Self::ContainsAny(needles) => needles.iter().any(|needle| contents.contains(needle)),
            Self::LineStartsWith(prefix) => contents
                .lines()
                .any(|line| line.trim_start().starts_with(prefix)),
            Self::NonEmpty => !contents.trim().is_empty(),
        }
    }
}

/// A missing file does not match.
pub async fn file_matches(file: &Path, test: ContentTest) -> Result<bool> {
    match read_to_string(file).await {
        Ok(contents) => Ok(test.matches(&contents)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(ProjectFinderError::read_file(file, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JS_WORKSPACE: ContentTest =
        ContentTest::ContainsAny(&["\"workspaces\"", "\"workspace\""]);

    #[test]
    fn contains_any_matches_a_single_needle() {
        assert!(JS_WORKSPACE.matches(r#"{"name": "x", "workspaces": ["a"]}"#));
        assert!(!JS_WORKSPACE.matches(r#"{"name": "x"}"#));
    }

    #[test]
    fn line_starts_with_ignores_position_in_file() {
        let test = ContentTest::LineStartsWith("[workspace]");
        assert!(test.matches("# a comment\n[workspace]\nmembers = []"));
        assert!(test.matches("[workspace]"));
        assert!(!test.matches("[workspace.dependencies]\n"));
        assert!(!test.matches("[package]\nname = \"x\""));
    }

    #[test]
    fn non_empty_ignores_whitespace_only_files() {
        assert!(ContentTest::NonEmpty.matches("{}"));
        assert!(!ContentTest::NonEmpty.matches("  \n\t\n"));
    }
}
