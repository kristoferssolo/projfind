//! Directory shapes the benchmarks scan.
//!
//! Every builder returns the package directories it created, so resolution
//! benchmarks can start from a marker instead of walking the tree again.

use color_eyre::eyre::Result;
use std::{
    fmt::Write as _,
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
};

/// Directories without a marker for every repository in a haystack.
const NOISE_PER_REPO: usize = 6;

/// Levels of empty nesting under each of those directories.
const NOISE_DEPTH: usize = 3;

/// Manifests each repository in `nested_projects` buries below its root.
const VENDORED: [&str; 3] = ["vendor/left", "vendor/right", "examples/demo"];

fn repository(dir: &Path) -> Result<()> {
    create_dir_all(dir.join(".git/objects"))?;
    write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    Ok(())
}

fn package(dir: &Path, name: &str) -> Result<PathBuf> {
    create_dir_all(dir.join("src"))?;
    write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n"),
    )?;
    write(dir.join("src/lib.rs"), "")?;
    Ok(dir.to_path_buf())
}

/// `count` sibling repositories, each holding exactly one package.
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

/// One repository whose Cargo workspace owns `members` member crates.
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

/// One repository with a single package buried `levels` directories down.
pub fn deep_nesting(root: &Path, levels: usize) -> Result<Vec<PathBuf>> {
    repository(root)?;

    let leaf = (0..levels).fold(root.to_path_buf(), |dir, level| {
        dir.join(format!("level-{level:02}"))
    });

    Ok(vec![package(&leaf, "buried")?])
}

/// `count` repositories hidden among directories that hold no marker at all.
///
/// A scan of a home directory spends most of its time here rather than on the
/// projects it reports, so this is the shape that exercises the walk itself.
pub fn haystack(root: &Path, count: usize) -> Result<Vec<PathBuf>> {
    let repos = flat_repos(&root.join("repos"), count)?;

    for index in 0..count * NOISE_PER_REPO {
        let dir = (0..NOISE_DEPTH)
            .fold(root.join(format!("noise/dir-{index:05}")), |dir, level| {
                dir.join(format!("part-{level}"))
            });
        create_dir_all(&dir)?;
        write(dir.join("notes.txt"), "")?;
    }

    Ok(repos)
}

/// `count` repositories that each bury three extra manifests below their root.
///
/// Every buried manifest is a candidate that resolution has to fold back into
/// the enclosing repository, so this shape costs four times the marker work of
/// `flat_repos` for the same number of reported projects.
pub fn nested_projects(root: &Path, count: usize) -> Result<Vec<PathBuf>> {
    (0..count)
        .map(|index| {
            let name = format!("repo-{index:04}");
            let dir = root.join(&name);
            repository(&dir)?;
            package(&dir, &name)?;
            for nested in VENDORED {
                package(&dir.join(nested), &name)?;
            }
            Ok(dir)
        })
        .collect()
}
