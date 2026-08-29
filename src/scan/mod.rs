//! Walking the search directories once, in parallel.

mod exclusion;

use crate::{
    error::Result,
    git::{GIT_DIR, marks_repository},
    scan::exclusion::Exclusions,
};
use ignore::{DirEntry, WalkBuilder, WalkState};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tracing::{debug, warn};

const POISONED: &str = "scan lock poisoned";

/// Everything one walk found: repository roots and paths to marker files.
#[derive(Debug, Default)]
pub struct DirectoryScan {
    pub git_repos: Vec<PathBuf>,
    pub marker_files: Vec<PathBuf>,
}

/// Finds repositories and project markers below `dirs`.
///
/// The walker follows symlinks, visits hidden entries, and respects ignore
/// files. Exclusion patterns use gitignore syntax relative to each search
/// directory. Unreadable entries are logged and skipped.
///
/// # Errors
///
/// Returns an error if an exclusion pattern cannot be parsed or compiled.
///
/// # Panics
///
/// Panics if a walker thread panicked while holding the result lock.
pub fn scan_directories(
    dirs: &[PathBuf],
    marker_names: &[String],
    max_depth: usize,
    exclude: &[String],
) -> Result<DirectoryScan> {
    let Some((first, rest)) = dirs.split_first() else {
        return Ok(DirectoryScan::default());
    };

    let visitor = Visitor {
        exclusions: Exclusions::new(dirs, exclude)?,
        markers: marker_names.iter().map(String::as_str).collect(),
        scan: Mutex::new(DirectoryScan::default()),
    };

    build_walker(first, rest, max_depth)
        .build_parallel()
        .run(|| Box::new(|entry| visitor.visit(entry)));

    Ok(visitor.scan.into_inner().expect(POISONED))
}

fn build_walker(first: &Path, rest: &[PathBuf], max_depth: usize) -> WalkBuilder {
    let mut builder = WalkBuilder::new(first);
    debug!("Scanning {}", first.display());
    for dir in rest {
        debug!("Scanning {}", dir.display());
        builder.add(dir);
    }
    builder
        .hidden(false)
        .follow_links(true)
        .max_depth(Some(max_depth));
    builder
}

/// What every walker thread needs, and the results they share.
struct Visitor<'a> {
    exclusions: Exclusions,
    markers: HashSet<&'a str>,
    scan: Mutex<DirectoryScan>,
}

impl Visitor<'_> {
    fn visit(&self, entry: std::result::Result<DirEntry, ignore::Error>) -> WalkState {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!("Skipping unreadable entry: {error}");
                return WalkState::Continue;
            }
        };

        // The search directories themselves are never results.
        if entry.depth() == 0 {
            return WalkState::Continue;
        }
        let Some(file_type) = entry.file_type() else {
            return WalkState::Continue;
        };
        if self.exclusions.matches(entry.path(), file_type.is_dir()) {
            return WalkState::Skip;
        }
        if !file_type.is_dir() && !file_type.is_file() {
            return WalkState::Continue;
        }

        if entry.file_name() == GIT_DIR {
            return self.record_repository(&entry);
        }
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| self.markers.contains(name))
        {
            return self.record_marker(entry);
        }

        WalkState::Continue
    }

    /// Records the directory holding a `.git` entry, and stops descending into
    /// the repository it marks.
    fn record_repository(&self, entry: &DirEntry) -> WalkState {
        let path = entry.path();
        match marks_repository(path) {
            Ok(true) => {}
            Ok(false) => return WalkState::Continue,
            Err(error) => {
                warn!("Skipping unreadable repository marker: {error}");
                return WalkState::Continue;
            }
        }

        if let Some(parent) = path.parent() {
            self.push(|scan| scan.git_repos.push(parent.to_path_buf()));
        }
        WalkState::Skip
    }

    fn record_marker(&self, entry: DirEntry) -> WalkState {
        // A marker that is itself a directory, such as a bare `rockspec`, has
        // nothing below it worth walking.
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        self.push(|scan| scan.marker_files.push(entry.into_path()));

        if is_dir {
            WalkState::Skip
        } else {
            WalkState::Continue
        }
    }

    fn push(&self, record: impl FnOnce(&mut DirectoryScan)) {
        record(&mut self.scan.lock().expect(POISONED));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use claims::assert_ok;
    use std::{fs, os::unix::net::UnixListener};
    use tempfile::TempDir;

    #[test]
    fn special_files_are_not_markers() {
        let temp = TempDir::new().expect("temp dir");
        let project = temp.path().join("project");
        fs::create_dir(&project).expect("create project");
        let _socket = UnixListener::bind(project.join("Cargo.toml")).expect("create socket");

        let scan = assert_ok!(scan_directories(
            &[temp.path().to_path_buf()],
            &["Cargo.toml".to_owned()],
            2,
            &[],
        ));

        assert!(scan.marker_files.is_empty());
        assert!(scan.git_repos.is_empty());
    }
}
