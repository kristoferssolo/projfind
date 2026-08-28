use ignore::{WalkBuilder, WalkState};
use std::{collections::HashSet, path::PathBuf, sync::Mutex};
use tracing::{debug, warn};

const GIT_DIR: &str = ".git";
const POISONED: &str = "scan lock poisoned";

/// Everything one walk found: repository roots and paths to marker files.
#[derive(Debug, Default)]
pub struct DirectoryScan {
    pub git_repos: Vec<PathBuf>,
    pub marker_files: Vec<PathBuf>,
}

/// Walks every directory in `dirs` at once, collecting repositories and any
/// file or directory named after one of `marker_names`.
///
/// Hidden entries are visited, symlinks are followed, and ignore files are
/// honoured. Repositories are never descended into, so a marker inside `.git`
/// cannot masquerade as a project. Entries that cannot be read are logged and
/// skipped rather than failing the whole walk.
///
/// # Panics
///
/// Panics if a walker thread panicked while holding the result lock.
#[must_use]
pub fn scan_directories(
    dirs: &[PathBuf],
    marker_names: &[String],
    max_depth: usize,
) -> DirectoryScan {
    let Some((first, rest)) = dirs.split_first() else {
        return DirectoryScan::default();
    };

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

    let markers = marker_names
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let scan = Mutex::new(DirectoryScan::default());

    builder.build_parallel().run(|| {
        Box::new(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warn!("Skipping unreadable entry: {error}");
                    return WalkState::Continue;
                }
            };

            // Depth zero is a search root, which never matches itself.
            if entry.depth() == 0 {
                return WalkState::Continue;
            }
            let Some(file_type) = entry.file_type() else {
                return WalkState::Continue;
            };
            if !file_type.is_dir() && !file_type.is_file() {
                return WalkState::Continue;
            }

            let name = entry.file_name();
            if name == GIT_DIR {
                // A `.git` file points at a worktree elsewhere; only the
                // directory marks a repository.
                if !file_type.is_dir() {
                    return WalkState::Continue;
                }
                if let Some(parent) = entry.path().parent() {
                    scan.lock()
                        .expect(POISONED)
                        .git_repos
                        .push(parent.to_path_buf());
                }
                return WalkState::Skip;
            }

            if name.to_str().is_some_and(|name| markers.contains(name)) {
                scan.lock()
                    .expect(POISONED)
                    .marker_files
                    .push(entry.into_path());
                if file_type.is_dir() {
                    return WalkState::Skip;
                }
            }

            WalkState::Continue
        })
    });

    scan.into_inner().expect(POISONED)
}
