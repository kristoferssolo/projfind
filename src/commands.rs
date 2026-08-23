use crate::{
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
};
use regex::escape;
use std::{
    collections::HashMap,
    fmt::Display,
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

/// Helper to wrap command errors in a uniform way.
fn wrap_command_error<E: Display>(action: &str, err: E) -> ProjectFinderError {
    ProjectFinderError::CommandExecutionFailed(format!("{action}: {err}"))
}

/// Run the `fd` command to find files matching one or more literal patterns.
///
/// The function builds a combined regex pattern from the list of patterns, runs the
/// command asynchronously, and collects matching file paths in a map keyed by the literal
/// file name.
///
/// # Arguments
///
/// - `deps`: Dependencies hold the path to the `fd` binary.
/// - `dir`: The directory in which to search.
/// - `patterns`: A list of file name patterns (literals) to match.
/// - `max_depth`: The maximum directory depth for the search.
///
/// # Returns
///
/// A map where each key is one of the patterns and the value is the list of matching
/// file paths.
pub async fn find_files(
    deps: &Dependencies,
    dir: &Path,
    patterns: &[&str],
    max_depth: usize,
) -> Result<HashMap<String, Vec<PathBuf>>> {
    // Build a regex pattern that matches any of the provided (literal) patterns.
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

    let mut child = cmd.spawn().map_err(|e| {
        ProjectFinderError::CommandExecutionFailed(format!("Failed to spawn fd: {e}"))
    })?;

    // Capture stdout and wrap it with a buffered reader.
    let stdout = child.stdout.take().ok_or_else(|| {
        ProjectFinderError::CommandExecutionFailed("Failed to capture stdout".into())
    })?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Prepare the results map with an empty vector for each pattern.
    let mut results = patterns
        .iter()
        .map(|pattern| ((*pattern).to_string(), Vec::new()))
        .collect::<HashMap<_, _>>();

    // Stream and process output as lines arrive.
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| wrap_command_error("Failed to read stdout", e))?
    {
        let path = PathBuf::from(line);
        // For each found file, only add it if its file name exactly matches one
        // of the provided patterns.
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            if let Some(entries) = results.get_mut(file_name) {
                entries.push(path);
            }
        }
    }

    // Wait for the command to finish.
    let status = child
        .wait()
        .await
        .map_err(|e| wrap_command_error("Failed to wait process", e))?;
    if !status.success() {
        warn!("fd command exited with non-zero status: {status}");
    }

    Ok(results)
}

/// Find Git repositories by searching for '.git' directories.
///
/// This function invokes the `fd` command with the pattern '^.git$'. For each
/// found directory, it returns the parent path (the Git repository root).
///
/// # Arguments
///
/// - `deps`: Dependencies containing the path to the `fd` binary.
/// - `dir`: The directory to search for Git repositories.
/// - `max_depth`: The maximum directory depth to search.
///
/// # Returns
///
/// A vector of paths representing the roots of Git repositories.
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
        .map_err(|e| wrap_command_error("Failed to find git repositories", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("fd command failed: {stderr}");
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).map_err(ProjectFinderError::Utf8Error)?;

    // For each found '.git' directory, return its parent directory.
    let paths = stdout
        .lines()
        .filter_map(|line| {
            let path = PathBuf::from(line);
            path.parent().map(std::path::Path::to_path_buf)
        })
        .collect();

    Ok(paths)
}

/// A test applied to a file's contents.
///
/// These replace the regexes the workspace rules used to carry: every pattern
/// in use was a plain substring or line-prefix check, so a regex engine bought
/// nothing but a compile on every call and a fallible path.
#[derive(Debug, Clone, Copy)]
pub enum ContentTest {
    /// Any of the given substrings appears somewhere in the file.
    ContainsAny(&'static [&'static str]),
    /// Some line, ignoring leading whitespace, starts with the given text.
    LineStartsWith(&'static str),
    /// The file holds something other than whitespace.
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

/// Read `file` into memory and check its contents against `test`.
///
/// A missing file is not an error, it simply fails the test. That lets callers
/// probe for optional config files with a single read rather than a stat
/// followed by a read.
pub async fn file_matches(file: &Path, test: ContentTest) -> Result<bool> {
    match read_to_string(file).await {
        Ok(contents) => Ok(test.matches(&contents)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(ProjectFinderError::CommandExecutionFailed(format!(
            "Failed to read file {}: {e}",
            file.display()
        ))),
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
        // The old `^\[workspace\]` regex was anchored to the start of the whole
        // file, so it missed any manifest with a comment or table above it.
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
