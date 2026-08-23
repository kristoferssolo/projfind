use color_eyre::eyre::Result;
use std::{
    fmt::Write as _,
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
};

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

pub fn deep_nesting(root: &Path, levels: usize) -> Result<Vec<PathBuf>> {
    repository(root)?;

    let leaf = (0..levels).fold(root.to_path_buf(), |dir, level| {
        dir.join(format!("level-{level:02}"))
    });

    Ok(vec![package(&leaf, "buried")?])
}
