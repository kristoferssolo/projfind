use color_eyre::eyre::{Result, bail};
use std::{
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_projfind");

fn sample_tree() -> Result<TempDir> {
    let temp = TempDir::new()?;
    let root = temp.path();

    create_dir_all(root.join("alpha/.git"))?;
    create_dir_all(root.join("beta"))?;
    write(root.join("beta/Cargo.toml"), "[package]\nname = \"beta\"\n")?;
    create_dir_all(root.join("gamma"))?;
    write(root.join("gamma/notes.txt"), "not a project")?;

    Ok(temp)
}

fn run(args: &[&str]) -> Result<Output> {
    Ok(Command::new(BIN).args(args).output()?)
}

fn found_projects(root: &Path, extra_args: &[&str]) -> Result<Vec<PathBuf>> {
    let root = root.to_string_lossy().into_owned();
    let mut args = vec![root.as_str()];
    args.extend_from_slice(extra_args);

    let output = run(&args)?;
    if !output.status.success() {
        bail!(
            "binary failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(|line| {
            Path::new(line)
                .strip_prefix(&root)
                .unwrap_or_else(|_| Path::new(line))
                .to_path_buf()
        })
        .collect())
}

#[test]
fn reports_git_repositories_and_marker_files() -> Result<()> {
    let temp = sample_tree()?;

    let projects = found_projects(temp.path(), &[])?;

    assert_eq!(projects, [PathBuf::from("alpha"), PathBuf::from("beta")]);
    Ok(())
}

#[test]
fn max_results_truncates_the_output() -> Result<()> {
    let temp = sample_tree()?;

    let projects = found_projects(temp.path(), &["--max-results", "1"])?;

    assert_eq!(projects, [PathBuf::from("alpha")]);
    Ok(())
}

#[test]
fn depth_zero_finds_nothing_below_the_root() -> Result<()> {
    let temp = sample_tree()?;

    let projects = found_projects(temp.path(), &["--depth", "0"])?;

    assert!(projects.is_empty(), "unexpectedly found {projects:?}");
    Ok(())
}

#[test]
fn a_missing_path_fails_with_a_readable_error() -> Result<()> {
    let output = run(&["/definitely/not/a/real/directory"])?;

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Path not found"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn zero_max_results_is_rejected() -> Result<()> {
    let output = run(&["--max-results", "0", "."])?;

    assert!(!output.status.success());
    Ok(())
}

#[test]
fn xdg_config_overrides_built_in_defaults() -> Result<()> {
    let temp = TempDir::new()?;
    let search_dir = temp.path().join("projects");
    let workspace = search_dir.join("workspace");
    create_dir_all(workspace.join("member"))?;
    write(workspace.join("workspace.root"), "")?;
    write(workspace.join("member/package.json"), "{}")?;

    let ignored = search_dir.join("ignored");
    create_dir_all(&ignored)?;
    write(
        ignored.join("Cargo.toml"),
        "[package]\nname = \"ignored\"\n",
    )?;

    let config_home = temp.path().join("config");
    let config_dir = config_home.join("projfind");
    create_dir_all(&config_dir)?;
    write(
        config_dir.join("config.toml"),
        format!(
            r#"
search_dirs = ["{}"]
marker_files = ["package.json"]
workspace_files = ["workspace.root"]
"#,
            search_dir.display()
        ),
    )?;

    let output = Command::new(BIN)
        .env("XDG_CONFIG_HOME", config_home)
        .output()?;

    if !output.status.success() {
        bail!(
            "binary failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let projects = String::from_utf8(output.stdout)?
        .lines()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    assert_eq!(projects, [workspace]);
    Ok(())
}
