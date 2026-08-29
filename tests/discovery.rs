mod common;

use claims::assert_err;
use color_eyre::eyre::{Result, eyre};
use common::{file, repository, worktree};
use mekle::{config::Config, error::Error, finder::ProjectFinder};
use std::{
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

fn search(root: &Path, config: Config) -> Result<Vec<PathBuf>> {
    let projects = ProjectFinder::new(config).find_projects()?;

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

fn projects_in(root: &Path) -> Result<Vec<PathBuf>> {
    search(root, config_for(root)?)
}

fn expect(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

#[test]
fn repositories_and_marker_directories_are_both_projects() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("cloned"))?;
    file(&temp.path().join("manifest/go.mod"), "module example\n")?;
    file(&temp.path().join("neither/README.md"), "not a project")?;

    assert_eq!(projects_in(temp.path())?, expect(&["cloned", "manifest"]));
    Ok(())
}

#[test]
fn a_worktree_is_a_project_without_a_marker() -> Result<()> {
    let temp = TempDir::new()?;
    let main = TempDir::new()?;
    worktree(
        &temp.path().join("feature"),
        &main.path().join(".git/worktrees/feature"),
    )?;

    assert_eq!(projects_in(temp.path())?, expect(&["feature"]));
    Ok(())
}

#[test]
fn a_git_file_that_redirects_nowhere_is_not_a_repository() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("dangling/.git"),
        "gitdir: /elsewhere/.git\n",
    )?;
    file(&temp.path().join("notes/.git"), "just a stray file\n")?;

    let projects = projects_in(temp.path())?;

    assert!(projects.is_empty(), "unexpectedly found {projects:?}");
    Ok(())
}

#[test]
fn nested_repositories_are_reported_separately() -> Result<()> {
    let temp = TempDir::new()?;
    repository(temp.path())?;
    repository(&temp.path().join("vendor/inner"))?;

    assert_eq!(projects_in(temp.path())?, expect(&["", "vendor/inner"]));
    Ok(())
}

#[test]
fn git_directories_are_not_searched_for_markers() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("repo"))?;
    file(
        &temp.path().join("repo/.git/modules/dep/Cargo.toml"),
        "[package]\nname = \"dep\"\n",
    )?;

    assert_eq!(projects_in(temp.path())?, expect(&["repo"]));
    Ok(())
}

#[test]
fn a_cargo_workspace_reports_only_its_root() -> Result<()> {
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

    assert_eq!(projects_in(temp.path())?, expect(&[""]));
    Ok(())
}

#[test]
fn a_cargo_workspace_absorbs_members_without_a_repository() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("Cargo.toml"),
        "# a comment first\n[workspace]\nmembers = [\"crates/*\"]\n",
    )?;
    file(
        &temp.path().join("crates/one/Cargo.toml"),
        "[package]\nname = \"one\"\n",
    )?;

    assert_eq!(projects_in(temp.path())?, expect(&[""]));
    Ok(())
}

#[test]
fn package_json_workspaces_absorb_their_members() -> Result<()> {
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

    assert_eq!(projects_in(temp.path())?, expect(&[""]));
    Ok(())
}

#[test]
fn a_configured_workspace_file_absorbs_members() -> Result<()> {
    let temp = TempDir::new()?;
    file(
        &temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )?;
    file(
        &temp.path().join("packages/ui/package.json"),
        r#"{"name": "ui"}"#,
    )?;

    assert_eq!(projects_in(temp.path())?, expect(&[""]));
    Ok(())
}

#[test]
fn a_direct_child_is_a_project_but_a_grandchild_is_not() -> Result<()> {
    let temp = TempDir::new()?;
    file(&temp.path().join("service/go.mod"), "module service\n")?;
    file(&temp.path().join("service/tool/go.mod"), "module tool\n")?;
    file(
        &temp.path().join("service/tool/plugin/go.mod"),
        "module plugin\n",
    )?;

    assert_eq!(
        projects_in(temp.path())?,
        expect(&["service", "service/tool"])
    );
    Ok(())
}

#[test]
fn build_files_resolve_to_the_outermost_one() -> Result<()> {
    let temp = TempDir::new()?;
    file(&temp.path().join("Makefile"), "all:\n")?;
    file(&temp.path().join("lib/Makefile"), "all:\n")?;
    file(&temp.path().join("lib/codec/Makefile"), "all:\n")?;

    assert_eq!(projects_in(temp.path())?, expect(&[""]));
    Ok(())
}

#[test]
fn symlinked_directories_are_followed() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("actual/checkout"))?;
    symlink("actual", temp.path().join("linked"))?;

    assert_eq!(
        projects_in(temp.path())?,
        expect(&["actual/checkout", "linked/checkout"])
    );
    Ok(())
}

#[test]
fn the_depth_limit_bounds_the_search() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("near"))?;
    repository(&temp.path().join("one/two/far"))?;

    let mut config = config_for(temp.path())?;
    config.depth = 2;

    assert_eq!(search(temp.path(), config)?, expect(&["near"]));
    Ok(())
}

#[test]
fn max_results_keeps_the_first_roots_in_order() -> Result<()> {
    let temp = TempDir::new()?;
    for name in ["charlie", "alpha", "bravo"] {
        repository(&temp.path().join(name))?;
    }

    let mut config = config_for(temp.path())?;
    config.max_results = Some(2.try_into()?);

    assert_eq!(search(temp.path(), config)?, expect(&["alpha", "bravo"]));
    Ok(())
}

#[test]
fn several_search_paths_are_merged() -> Result<()> {
    let first = TempDir::new()?;
    let second = TempDir::new()?;
    repository(&first.path().join("from-first"))?;
    repository(&second.path().join("from-second"))?;

    let mut config = config_for(first.path())?;
    config.paths = vec![first.path().to_path_buf(), second.path().to_path_buf()];

    let projects = ProjectFinder::new(config).find_projects()?;

    let mut expected = vec![
        first.path().join("from-first"),
        second.path().join("from-second"),
    ];
    expected.sort_unstable();

    assert_eq!(projects, expected);
    Ok(())
}

#[test]
fn a_search_path_that_is_not_a_directory_fails() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("nowhere");

    let mut config = config_for(temp.path())?;
    config.paths = vec![missing.clone()];

    let error = assert_err!(ProjectFinder::new(config).find_projects());

    assert!(
        matches!(&error, Error::PathNotFound(path) if *path == missing),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn an_excluded_directory_is_not_a_project() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("keep"))?;
    repository(&temp.path().join("target/inside"))?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["target/".to_owned()];

    assert_eq!(search(temp.path(), config)?, expect(&["keep"]));
    Ok(())
}

#[test]
fn an_excluded_worktree_is_not_a_project() -> Result<()> {
    let temp = TempDir::new()?;
    let main = TempDir::new()?;
    repository(&temp.path().join("keep"))?;
    worktree(
        &temp.path().join("worktrees/feature"),
        &main.path().join(".git/worktrees/feature"),
    )?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["worktrees/".to_owned()];

    assert_eq!(search(temp.path(), config)?, expect(&["keep"]));
    Ok(())
}

#[test]
fn an_excluded_marker_file_is_not_a_project() -> Result<()> {
    let temp = TempDir::new()?;
    file(&temp.path().join("keep/Cargo.toml"), "[package]\n")?;
    file(&temp.path().join("generated/Cargo.toml"), "[package]\n")?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["generated/".to_owned()];

    assert_eq!(search(temp.path(), config)?, expect(&["keep"]));
    Ok(())
}

#[test]
fn anchored_patterns_exclude_only_at_the_search_root() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("archive/repo"))?;
    repository(&temp.path().join("nested/archive/repo"))?;
    repository(&temp.path().join("repo"))?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["/archive/".to_owned()];

    assert_eq!(
        search(temp.path(), config)?,
        expect(&["nested/archive/repo", "repo"])
    );
    Ok(())
}

#[test]
fn recursive_patterns_exclude_at_any_depth() -> Result<()> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("app/vendor/repo"))?;
    repository(&temp.path().join("repo"))?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["**/vendor/".to_owned()];

    assert_eq!(search(temp.path(), config)?, expect(&["repo"]));
    Ok(())
}

#[test]
fn exclusions_apply_to_every_search_directory() -> Result<()> {
    let temp = TempDir::new()?;
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    repository(&first.join("skip/repo"))?;
    repository(&first.join("keep/repo"))?;
    repository(&second.join("skip/repo"))?;
    repository(&second.join("keep/repo"))?;

    let mut config = config_for(temp.path())?;
    config.paths = vec![first, second];
    config.exclude = vec!["skip/".to_owned()];

    let projects = ProjectFinder::new(config).find_projects()?;

    assert_eq!(projects.len(), 2);
    assert!(projects.iter().all(|project| {
        project
            .file_name()
            .is_some_and(|name| name == "keep" || name == "repo")
    }));
    Ok(())
}

#[test]
fn exclusions_stack_with_ignore_files() -> Result<()> {
    let temp = TempDir::new()?;
    // Ignore files are only honoured inside a repository.
    repository(temp.path())?;
    repository(&temp.path().join("excluded/repo"))?;
    repository(&temp.path().join("ignored/repo"))?;
    repository(&temp.path().join("repo"))?;
    file(&temp.path().join(".gitignore"), "ignored/\n")?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["excluded/".to_owned()];

    assert_eq!(search(temp.path(), config)?, expect(&["", "repo"]));
    Ok(())
}

#[test]
fn an_invalid_exclusion_pattern_fails_the_search() -> Result<()> {
    let temp = TempDir::new()?;

    let mut config = config_for(temp.path())?;
    config.exclude = vec!["[z-a]".to_owned()];

    let error = assert_err!(ProjectFinder::new(config).find_projects());

    assert!(
        error.to_string().contains("[z-a]"),
        "error does not name the pattern: {error}"
    );
    Ok(())
}
