use claims::assert_err;
use color_eyre::eyre::{Result, eyre};
use mekle::{
    config::Config, dependencies::Dependencies, errors::ProjectFinderError, finder::ProjectFinder,
};
use std::{
    fs::{create_dir_all, write},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

const TEST_DEPTH: usize = 12;

fn config_for(root: &Path) -> Result<Config> {
    let mut config = Config::defaults()?;
    config.paths = vec![root.to_path_buf()];
    config.depth = TEST_DEPTH;
    Ok(config)
}

async fn search(root: &Path, config: Config) -> Result<Vec<PathBuf>> {
    let projects = ProjectFinder::new(config, Dependencies::check()?)
        .find_projects()
        .await?;

    projects
        .iter()
        .map(|project| {
            project
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|_| eyre!("{} escaped the search root", project.display()))
        })
        .collect()
}

async fn projects_in(root: &Path) -> Result<Vec<PathBuf>> {
    search(root, config_for(root)?).await
}

fn expect(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn repository(dir: &Path) -> Result<()> {
    create_dir_all(dir.join(".git"))?;
    write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    Ok(())
}

fn file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    write(path, contents)?;
    Ok(())
}

#[tokio::test]
async fn repositories_and_marker_directories_are_both_projects() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("cloned"))?;
    file(&temp.path().join("manifest/go.mod"), "module example\n")?;
    file(&temp.path().join("neither/README.md"), "not a project")?;

    assert_eq!(
        projects_in(temp.path()).await?,
        expect(&["cloned", "manifest"])
    );
    Ok(())
}

#[tokio::test]
async fn a_git_file_is_not_a_repository() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("worktree/.git"),
        "gitdir: /elsewhere/.git\n",
    )?;

    let projects = projects_in(temp.path()).await?;

    assert!(projects.is_empty(), "unexpectedly found {projects:?}");
    Ok(())
}

#[tokio::test]
async fn nested_repositories_are_reported_separately() -> Result<()> {
    let temp = TempDir::new()?;
    repository(temp.path())?;
    repository(&temp.path().join("vendor/inner"))?;

    assert_eq!(
        projects_in(temp.path()).await?,
        expect(&["", "vendor/inner"])
    );
    Ok(())
}

#[tokio::test]
async fn git_directories_are_not_searched_for_markers() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("repo"))?;
    file(
        &temp.path().join("repo/.git/modules/dep/Cargo.toml"),
        "[package]\nname = \"dep\"\n",
    )?;

    assert_eq!(projects_in(temp.path()).await?, expect(&["repo"]));
    Ok(())
}

#[tokio::test]
async fn a_cargo_workspace_reports_only_its_root() -> Result<()> {
    let temp = TempDir::new()?;
    repository(temp.path())?;
    file(
        &temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )?;
    file(
        &temp.path().join("crates/one/Cargo.toml"),
        "[package]\nname = \"one\"\n",
    )?;
    file(
        &temp.path().join("crates/two/Cargo.toml"),
        "[package]\nname = \"two\"\n",
    )?;

    assert_eq!(projects_in(temp.path()).await?, expect(&[""]));
    Ok(())
}

#[tokio::test]
async fn a_cargo_workspace_absorbs_members_without_a_repository() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("Cargo.toml"),
        "# a comment first\n[workspace]\nmembers = [\"crates/*\"]\n",
    )?;
    file(
        &temp.path().join("crates/one/Cargo.toml"),
        "[package]\nname = \"one\"\n",
    )?;

    assert_eq!(projects_in(temp.path()).await?, expect(&[""]));
    Ok(())
}

#[tokio::test]
async fn package_json_workspaces_absorb_their_members() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("package.json"),
        r#"{"name": "root", "workspaces": ["packages/*"]}"#,
    )?;
    file(
        &temp.path().join("packages/ui/package.json"),
        r#"{"name": "ui"}"#,
    )?;
    file(
        &temp.path().join("packages/api/package.json"),
        r#"{"name": "api"}"#,
    )?;

    assert_eq!(projects_in(temp.path()).await?, expect(&[""]));
    Ok(())
}

#[tokio::test]
async fn a_configured_workspace_file_absorbs_members() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )?;
    file(
        &temp.path().join("packages/ui/package.json"),
        r#"{"name": "ui"}"#,
    )?;

    assert_eq!(projects_in(temp.path()).await?, expect(&[""]));
    Ok(())
}

#[tokio::test]
async fn a_direct_child_is_a_project_but_a_grandchild_is_not() -> Result<()> {
    let temp = TempDir::new()?;
    file(&temp.path().join("service/go.mod"), "module service\n")?;
    file(&temp.path().join("service/tool/go.mod"), "module tool\n")?;
    file(
        &temp.path().join("service/tool/plugin/go.mod"),
        "module plugin\n",
    )?;

    assert_eq!(
        projects_in(temp.path()).await?,
        expect(&["service", "service/tool"])
    );
    Ok(())
}

#[tokio::test]
async fn build_files_resolve_to_the_outermost_one() -> Result<()> {
    let temp = TempDir::new()?;
    file(&temp.path().join("Makefile"), "all:\n")?;
    file(&temp.path().join("lib/Makefile"), "all:\n")?;
    file(&temp.path().join("lib/codec/Makefile"), "all:\n")?;

    assert_eq!(projects_in(temp.path()).await?, expect(&[""]));
    Ok(())
}

#[tokio::test]
async fn symlinked_directories_are_followed() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("actual/checkout"))?;
    symlink("actual", temp.path().join("linked"))?;

    assert_eq!(
        projects_in(temp.path()).await?,
        expect(&["actual/checkout", "linked/checkout"])
    );
    Ok(())
}

#[tokio::test]
async fn the_depth_limit_bounds_the_search() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("near"))?;
    repository(&temp.path().join("one/two/far"))?;

    let mut config = config_for(temp.path())?;
    config.depth = 2;

    assert_eq!(search(temp.path(), config).await?, expect(&["near"]));
    Ok(())
}

#[tokio::test]
async fn max_results_keeps_the_first_roots_in_order() -> Result<()> {
    let temp = TempDir::new()?;
    for name in ["charlie", "alpha", "bravo"] {
        repository(&temp.path().join(name))?;
    }

    let mut config = config_for(temp.path())?;
    config.max_results = Some(2.try_into()?);

    assert_eq!(
        search(temp.path(), config).await?,
        expect(&["alpha", "bravo"])
    );
    Ok(())
}

#[tokio::test]
async fn several_search_paths_are_merged() -> Result<()> {
    let first = TempDir::new()?;
    let second = TempDir::new()?;
    repository(&first.path().join("from-first"))?;
    repository(&second.path().join("from-second"))?;

    let mut config = config_for(first.path())?;
    config.paths = vec![first.path().to_path_buf(), second.path().to_path_buf()];

    let projects = ProjectFinder::new(config, Dependencies::check()?)
        .find_projects()
        .await?;

    let mut expected = vec![
        first.path().join("from-first"),
        second.path().join("from-second"),
    ];
    expected.sort_unstable();

    assert_eq!(projects, expected);
    Ok(())
}

#[tokio::test]
async fn a_search_path_that_is_not_a_directory_fails() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("nowhere");

    let mut config = config_for(temp.path())?;
    config.paths = vec![missing.clone()];

    let error = assert_err!(
        ProjectFinder::new(config, Dependencies::check()?)
            .find_projects()
            .await
    );

    assert!(
        matches!(&error, ProjectFinderError::PathNotFound(path) if *path == missing),
        "unexpected error: {error}"
    );
    Ok(())
}
