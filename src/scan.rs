use crate::errors::{ProjectFinderError, Result};
use ignore::{
    Match, WalkBuilder, WalkState,
    gitignore::{Gitignore, GitignoreBuilder},
};
use std::{collections::HashSet, path::Path, path::PathBuf, sync::Mutex};
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
/// `exclude` holds gitignore-style patterns interpreted relative to every
/// search directory. Matching directories are pruned and matching files are
/// skipped, in addition to whatever ignore files the walker honours, so
/// exclusions can only remove results. Hidden entries are visited, symlinks
/// are followed, and ignore files are honoured. Repositories are never
/// descended into, so a marker inside `.git` cannot masquerade as a project.
/// Entries that cannot be read are logged and skipped rather than failing the
/// whole walk.
///
/// # Errors
///
/// Returns an error if an exclusion pattern is invalid.
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

    // One matcher per search directory, so anchored patterns resolve against
    // the directory they belong to.
    let excluders = dirs
        .iter()
        .map(|dir| build_excluder(dir, exclude))
        .collect::<Result<Vec<_>>>()?;

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
        let scan = &scan;
        let markers = &markers;
        let excluders = &excluders;
        Box::new(move |entry| {
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

            if is_excluded(entry.path(), file_type.is_dir(), excluders) {
                // `Skip` prunes an excluded directory's descendants.
                return WalkState::Skip;
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

    Ok(scan.into_inner().expect(POISONED))
}

fn build_excluder(root: &Path, exclude: &[String]) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in exclude {
        builder.add_line(None, pattern).map_err(|source| {
            ProjectFinderError::InvalidExcludePattern {
                pattern: pattern.clone(),
                source,
            }
        })?;
    }

    builder
        .build()
        .map_err(|source| ProjectFinderError::InvalidExcludePattern {
            pattern: String::new(),
            source,
        })
}

/// Reports whether `path` matches an exclusion from any search directory that
/// contains it. Only `Match::Ignore` excludes, so an exclusion never
/// reinstates a path that another source ignored.
fn is_excluded(path: &Path, is_dir: bool, excluders: &[Gitignore]) -> bool {
    excluders
        .iter()
        .any(|excluder| matches!(excluder.matched(path, is_dir), Match::Ignore(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use std::fs;

    fn create(path: &Path) {
        fs::create_dir_all(path).expect("create directory");
    }

    fn write_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, "").expect("write file");
    }

    fn scan(root: &Path, exclude: &[&str]) -> Result<DirectoryScan> {
        scan_directories(
            &[root.to_path_buf()],
            &["Cargo.toml".to_owned()],
            12,
            &exclude
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    fn names(entries: &[PathBuf]) -> Vec<&str> {
        entries
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .collect()
    }

    #[test]
    fn an_excluded_directory_is_pruned() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path();
        create(&root.join("keep/repo/.git"));
        create(&root.join("skip/repo/.git"));

        let results = assert_ok!(scan(root, &["skip/"]));

        assert_eq!(names(&results.git_repos), ["repo"]);
        assert_eq!(
            results.git_repos[0].parent().map(Path::as_os_str),
            Some(root.join("keep").as_os_str())
        );
    }

    #[test]
    fn an_excluded_marker_file_is_ignored() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path();
        write_file(&root.join("keep/Cargo.toml"));
        write_file(&root.join("skip/Cargo.toml"));

        let results = assert_ok!(scan(root, &["skip/Cargo.toml"]));

        assert_eq!(names(&results.marker_files), ["Cargo.toml"]);
        assert_eq!(
            results.marker_files[0].parent().map(Path::as_os_str),
            Some(root.join("keep").as_os_str())
        );
    }

    #[test]
    fn anchored_and_recursive_patterns_both_apply() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path();
        create(&root.join("archive/repo/.git"));
        create(&root.join("nested/vendor/repo/.git"));
        create(&root.join("keep/repo/.git"));

        let results = assert_ok!(scan(root, &["/archive/", "**/vendor/"]));

        assert_eq!(names(&results.git_repos), ["repo"]);
        assert_eq!(
            results.git_repos[0].parent().map(Path::as_os_str),
            Some(root.join("keep").as_os_str())
        );
    }

    #[test]
    fn patterns_apply_to_each_search_directory() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        create(&first.join("archive/repo/.git"));
        create(&first.join("keep/repo/.git"));
        create(&second.join("archive/repo/.git"));
        create(&second.join("keep/repo/.git"));

        let results = assert_ok!(scan_directories(
            &[first, second],
            &[],
            12,
            &["/archive/".to_owned()],
        ));

        let mut parents = results
            .git_repos
            .iter()
            .map(|path| path.parent().expect("parent").file_name().expect("name"))
            .collect::<Vec<_>>();
        parents.sort_unstable();
        assert_eq!(parents, ["keep", "keep"]);
    }

    #[test]
    fn an_invalid_pattern_is_rejected_with_the_pattern_in_the_error() {
        let temp = tempfile::TempDir::new().expect("temp dir");

        let error = assert_err!(scan(temp.path(), &["[z-a]"]));

        assert!(
            error.to_string().contains("[z-a]"),
            "error does not name the pattern: {error}"
        );
    }

    #[test]
    fn empty_patterns_scan_everything() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path();
        create(&root.join("repo/.git"));
        write_file(&root.join("repo/Cargo.toml"));

        let results = assert_ok!(scan(root, &[]));

        assert_eq!(names(&results.git_repos), ["repo"]);
        assert_eq!(names(&results.marker_files), ["Cargo.toml"]);
    }
}
