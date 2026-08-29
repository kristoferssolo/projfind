use crate::errors::{ProjectFinderError, Result};
use std::{
    fs::{metadata, read_to_string},
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// The entry Git places at the root of every repository.
pub const GIT_DIR: &str = ".git";

const GITDIR_PREFIX: &str = "gitdir:";

/// Reports whether `git_path`, a path named [`GIT_DIR`], marks a repository
/// root.
///
/// A directory always marks one. A file marks one only when it is a Git
/// redirection file, as worktrees and some submodules use: a lone
/// `gitdir: <PATH>` entry naming an existing directory. A relative `<PATH>`
/// resolves against the directory holding the file.
///
/// # Errors
///
/// Returns an error if `git_path` or its target cannot be inspected. A missing
/// path is not an error; it simply marks nothing.
pub fn marks_repository(git_path: &Path) -> Result<bool> {
    match metadata(git_path) {
        Ok(entry) if entry.is_dir() => Ok(true),
        Ok(entry) if entry.is_file() => links_to_repository(git_path),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ProjectFinderError::read_file(git_path, error)),
    }
}

fn links_to_repository(git_file: &Path) -> Result<bool> {
    let contents = match read_to_string(git_file) {
        Ok(contents) => contents,
        // A redirection file is short text; binary contents cannot be one.
        Err(error) if error.kind() == ErrorKind::InvalidData => return Ok(false),
        Err(error) => return Err(ProjectFinderError::read_file(git_file, error)),
    };

    let Some(target) = link_target(&contents).and_then(|target| resolve(git_file, target)) else {
        return Ok(false);
    };

    is_dir(&target)
}

/// Extracts the target of a `gitdir:` entry that stands alone in `contents`.
fn link_target(contents: &str) -> Option<&str> {
    let mut lines = contents.lines();
    let target = lines.next()?.strip_prefix(GITDIR_PREFIX)?.trim();

    if target.is_empty() || lines.any(|line| !line.trim().is_empty()) {
        return None;
    }

    Some(target)
}

fn resolve(git_file: &Path, target: &str) -> Option<PathBuf> {
    let target = Path::new(target);
    if target.is_absolute() {
        return Some(target.to_path_buf());
    }

    Some(git_file.parent()?.join(target))
}

fn is_dir(target: &Path) -> Result<bool> {
    match metadata(target) {
        Ok(entry) => Ok(entry.is_dir()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ProjectFinderError::read_file(target, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_ok_eq, assert_some_eq};
    use rstest::rstest;
    use std::fs::{create_dir_all, write};
    use tempfile::TempDir;

    /// Builds a tree holding a directory target, a file target, and a
    /// `worktree/.git` file whose contents are `contents` with `<ROOT>`
    /// replaced by the tree's path. Returns the path of that file.
    fn git_file(root: &Path, contents: &str) -> PathBuf {
        create_dir_all(root.join("admin")).expect("create target dir");
        write(root.join("plain"), "not a directory").expect("write target file");
        let worktree = root.join("worktree");
        create_dir_all(&worktree).expect("create worktree dir");

        let git_file = worktree.join(GIT_DIR);
        let root = root.display().to_string();
        write(&git_file, contents.replace("<ROOT>", &root)).expect("write .git file");
        git_file
    }

    #[test]
    fn a_git_directory_marks_a_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let git_dir = temp.path().join(GIT_DIR);
        create_dir_all(&git_dir).expect("create .git");

        assert_ok_eq!(marks_repository(&git_dir), true);
    }

    #[test]
    fn a_missing_git_entry_marks_nothing() {
        let temp = TempDir::new().expect("create temp dir");

        assert_ok_eq!(marks_repository(&temp.path().join(GIT_DIR)), false);
    }

    #[rstest]
    #[case::absolute_target("gitdir: <ROOT>/admin\n", true)]
    #[case::relative_target("gitdir: ../admin\n", true)]
    #[case::crlf("gitdir: <ROOT>/admin\r\n", true)]
    #[case::no_trailing_newline("gitdir: <ROOT>/admin", true)]
    #[case::unrelated_contents("ref: refs/heads/main\n", false)]
    #[case::empty_target("gitdir:\n", false)]
    #[case::blank_target("gitdir:    \n", false)]
    #[case::extra_line("gitdir: <ROOT>/admin\nworktree: <ROOT>\n", false)]
    #[case::missing_target("gitdir: <ROOT>/nowhere\n", false)]
    #[case::file_target("gitdir: <ROOT>/plain\n", false)]
    fn a_git_file_marks_a_repository_only_when_it_redirects(
        #[case] contents: &str,
        #[case] expected: bool,
    ) {
        let temp = TempDir::new().expect("create temp dir");
        let git_file = git_file(temp.path(), contents);

        assert_ok_eq!(marks_repository(&git_file), expected);
    }

    #[test]
    fn a_trailing_blank_line_is_not_an_extra_entry() {
        assert_some_eq!(
            link_target("gitdir: /repo/.git/worktrees/one\n\n"),
            "/repo/.git/worktrees/one"
        );
    }
}
