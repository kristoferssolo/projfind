mod exclusion;

use crate::{
    errors::Result,
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

    let exclusions = Exclusions::new(dirs, exclude)?;
    let markers = marker_names.iter().map(String::as_str).collect();
    let scan = Mutex::new(DirectoryScan::default());
    let walker = build_walker(first, rest, max_depth);

    walker.build_parallel().run(|| {
        let exclusions = &exclusions;
        let markers = &markers;
        let scan = &scan;
        Box::new(move |entry| visit(entry, exclusions, markers, scan))
    });

    Ok(scan.into_inner().expect(POISONED))
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

fn visit(
    entry: std::result::Result<DirEntry, ignore::Error>,
    exclusions: &Exclusions,
    markers: &HashSet<&str>,
    scan: &Mutex<DirectoryScan>,
) -> WalkState {
    let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
            warn!("Skipping unreadable entry: {error}");
            return WalkState::Continue;
        }
    };

    let Some(file_type) = entry.file_type() else {
        return WalkState::Continue;
    };
    if entry.depth() == 0 {
        return WalkState::Continue;
    }
    if exclusions.matches(entry.path(), file_type.is_dir()) {
        return WalkState::Skip;
    }
    if !file_type.is_dir() && !file_type.is_file() {
        return WalkState::Continue;
    }

    if entry.file_name() == GIT_DIR {
        return record_repository(&entry, scan);
    }
    if entry
        .file_name()
        .to_str()
        .is_some_and(|name| markers.contains(name))
    {
        return record_marker(entry, scan);
    }

    WalkState::Continue
}

fn record_repository(entry: &DirEntry, scan: &Mutex<DirectoryScan>) -> WalkState {
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
        scan.lock()
            .expect(POISONED)
            .git_repos
            .push(parent.to_path_buf());
    }
    WalkState::Skip
}

fn record_marker(entry: DirEntry, scan: &Mutex<DirectoryScan>) -> WalkState {
    let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
    scan.lock()
        .expect(POISONED)
        .marker_files
        .push(entry.into_path());

    if is_dir {
        WalkState::Skip
    } else {
        WalkState::Continue
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
