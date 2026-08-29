use color_eyre::eyre::Result;
use std::{
    fs::{create_dir_all, write},
    path::Path,
};

/// Creates an ordinary Git repository at `dir`.
pub fn repository(dir: &Path) -> Result<()> {
    create_dir_all(dir.join(".git"))?;
    write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    Ok(())
}

/// Creates a linked worktree at `dir`, backed by the administrative directory
/// `gitdir`, the way `git worktree add` lays one out.
pub fn worktree(dir: &Path, gitdir: &Path) -> Result<()> {
    create_dir_all(gitdir)?;
    write(gitdir.join("HEAD"), "ref: refs/heads/feature\n")?;
    create_dir_all(dir)?;
    write(dir.join(".git"), format!("gitdir: {}\n", gitdir.display()))?;
    Ok(())
}

/// Writes `contents` to `path`, creating the directories above it.
pub fn file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    write(path, contents)?;
    Ok(())
}
