//! Deterministic project trees to benchmark against.
//!
//! Building the tree from code rather than replaying a recorded directory
//! listing keeps every run, on every machine, measuring the same shape, and
//! leaves a single knob to scale it with.
//!
//! Each builder returns the directories holding a marker file, which is what
//! the root resolver is handed in a real scan.

use color_eyre::eyre::Result;
use std::{
    fmt::Write as _,
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
};

/// Enough of a repository for the scanner: `.git` is a directory, which is the
/// only part [`projfind`] inspects, and it holds a file so pruning has
/// something to skip past.
fn repository(dir: &Path) -> Result<()> {
    create_dir_all(dir.join(".git/objects"))?;
    write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    Ok(())
}

/// A crate: the manifest that marks it, plus a source file so the walk has
/// ordinary entries to step over rather than markers alone.
fn package(dir: &Path, name: &str) -> Result<PathBuf> {
    create_dir_all(dir.join("src"))?;
    write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n"),
    )?;
    write(dir.join("src/lib.rs"), "")?;
    Ok(dir.to_path_buf())
}

/// `count` sibling repositories, each standing on its own: the shape of an
/// ordinary `~/repos` directory, and the case where nothing can be shared
/// between resolutions.
pub fn flat_repos(root: &Path, count: usize) -> Result<Vec<PathBuf>> {
    (0..count)
        .map(|index| {
            let name = format!("repo-{index:04}");
            let dir = root.join(&name);
            repository(&dir)?;
            package(&dir, &name)
        })
        .collect()
}

/// One repository whose workspace manifest owns `members` member crates. Every
/// member resolves to the same root, which is the case the resolver's
/// memoisation exists for.
pub fn monorepo(root: &Path, members: usize) -> Result<Vec<PathBuf>> {
    repository(root)?;

    let names = (0..members)
        .map(|index| format!("member-{index:04}"))
        .collect::<Vec<_>>();
    let mut manifest = String::from("[workspace]\nresolver = \"3\"\nmembers = [\n");
    for name in &names {
        writeln!(manifest, "  \"crates/{name}\",")?;
    }
    manifest.push_str("]\n");
    write(root.join("Cargo.toml"), manifest)?;

    names
        .iter()
        .map(|name| package(&root.join("crates").join(name), name))
        .collect()
}

/// A single crate buried `levels` directories below a repository root, so a
/// cold resolution has to climb the whole way before anything stops it.
pub fn deep_nesting(root: &Path, levels: usize) -> Result<Vec<PathBuf>> {
    repository(root)?;

    let leaf = (0..levels).fold(root.to_path_buf(), |dir, level| {
        dir.join(format!("level-{level:02}"))
    });

    Ok(vec![package(&leaf, "buried")?])
}
