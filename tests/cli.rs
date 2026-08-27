use color_eyre::eyre::{Result, bail};
use std::{
    ffi::{OsStr, OsString},
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_projfind");

struct Run {
    config_home: TempDir,
    home: Option<PathBuf>,
    args: Vec<OsString>,
}

impl Run {
    fn new() -> Result<Self> {
        Ok(Self {
            config_home: TempDir::new()?,
            home: None,
            args: Vec::new(),
        })
    }

    fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    fn config(self, contents: &str) -> Result<Self> {
        let dir = self.config_home.path().join("projfind");
        create_dir_all(&dir)?;
        write(dir.join("config.toml"), contents)?;
        Ok(self)
    }

    fn home(mut self, home: &Path) -> Self {
        self.home = Some(home.to_path_buf());
        self
    }

    fn output(&self) -> Result<Output> {
        let mut command = Command::new(BIN);
        command
            .args(&self.args)
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("XDG_DATA_HOME", self.config_home.path());

        match &self.home {
            Some(home) => command.env("HOME", home),
            None => command.env_remove("HOME"),
        };

        Ok(command.output()?)
    }

    fn projects(&self) -> Result<Vec<String>> {
        Ok(self.stdout()?.lines().map(str::to_owned).collect())
    }

    fn stdout(&self) -> Result<String> {
        let output = self.output()?;
        if !output.status.success() {
            bail!(
                "binary failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    fn failure(&self) -> Result<String> {
        let output = self.output()?;
        if output.status.success() {
            bail!(
                "binary unexpectedly succeeded: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stderr).into_owned())
    }

    fn clear_args(mut self) -> Self {
        self.args.clear();
        self
    }
}

fn repository(dir: &Path) -> Result<()> {
    create_dir_all(dir.join(".git"))?;
    write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    Ok(())
}

fn sample_tree() -> Result<TempDir> {
    let temp = TempDir::new()?;
    repository(&temp.path().join("alpha"))?;
    create_dir_all(temp.path().join("beta"))?;
    write(
        temp.path().join("beta/Cargo.toml"),
        "[package]\nname = \"beta\"\n",
    )?;
    Ok(temp)
}

fn joined(root: &Path, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| root.join(name).display().to_string())
        .collect()
}

#[test]
fn projects_are_printed_one_absolute_path_per_line() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?.arg(temp.path()).projects()?;

    assert_eq!(projects, joined(temp.path(), &["alpha", "beta"]));
    Ok(())
}

#[test]
fn max_results_truncates_the_output() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?
        .arg(temp.path())
        .arg("--max-results")
        .arg("1")
        .projects()?;

    assert_eq!(projects, joined(temp.path(), &["alpha"]));
    Ok(())
}

#[test]
fn recorded_projects_are_ranked_before_untracked_projects() -> Result<()> {
    let temp = sample_tree()?;
    let run = Run::new()?.home(temp.path()).arg("add").arg("~/beta");

    let output = run.output()?;
    assert!(
        output.status.success(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let projects = run
        .clear_args()
        .arg(temp.path())
        .arg("--max-results")
        .arg("1")
        .projects()?;

    assert_eq!(projects, ["~/beta"]);
    Ok(())
}

#[test]
fn history_list_and_show_report_project_scores() -> Result<()> {
    let temp = sample_tree()?;
    let run = Run::new()?.home(temp.path()).arg("add").arg("~/beta");
    run.stdout()?;

    let run = run.clear_args().arg("history").arg("list");
    let fields = run
        .stdout()?
        .trim()
        .split('\t')
        .map(str::to_owned)
        .collect::<Vec<_>>();

    assert_eq!(fields[0], "1");
    assert_eq!(fields[1], "4");
    assert!(fields[2].ends_with('s'), "unexpected age: {}", fields[2]);
    assert_eq!(fields[3], "~/beta");

    let run = run.clear_args().arg("history").arg("show").arg("~/beta");
    let output = run.stdout()?;
    assert!(output.ends_with("\t~/beta\n"));
    Ok(())
}

#[test]
fn history_scores_can_be_set_and_adjusted() -> Result<()> {
    let temp = sample_tree()?;
    let run = Run::new()?
        .home(temp.path())
        .arg("history")
        .arg("set")
        .arg("~/beta")
        .arg("10");
    run.stdout()?;

    let run = run
        .clear_args()
        .arg("history")
        .arg("adjust")
        .arg("~/beta")
        .arg("-3");
    run.stdout()?;

    let run = run.clear_args().arg("history").arg("show").arg("~/beta");
    let output = run.stdout()?;
    assert!(output.starts_with("7\t28\t"), "unexpected output: {output}");

    let run = run
        .clear_args()
        .arg("history")
        .arg("adjust")
        .arg("~/beta")
        .arg("-7");
    run.stdout()?;
    assert!(
        run.clear_args()
            .arg("history")
            .arg("list")
            .stdout()?
            .is_empty()
    );
    Ok(())
}

#[test]
fn history_entries_can_be_removed_individually_or_all_at_once() -> Result<()> {
    let temp = sample_tree()?;
    let run = Run::new()?.home(temp.path()).arg("add").arg("~/alpha");
    run.stdout()?;
    let run = run.clear_args().arg("add").arg("~/beta");
    run.stdout()?;

    let run = run.clear_args().arg("history").arg("remove").arg("~/alpha");
    run.stdout()?;
    let run = run.clear_args().arg("history").arg("list");
    let output = run.stdout()?;
    assert!(!output.contains("~/alpha"));
    assert!(output.contains("~/beta"));

    let run = run.clear_args().arg("history").arg("clear");
    run.stdout()?;
    assert!(
        run.clear_args()
            .arg("history")
            .arg("list")
            .stdout()?
            .is_empty()
    );
    Ok(())
}

#[test]
fn depth_zero_finds_nothing_below_the_root() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?
        .arg(temp.path())
        .arg("--depth")
        .arg("0")
        .projects()?;

    assert!(projects.is_empty(), "unexpectedly found {projects:?}");
    Ok(())
}

#[test]
fn zero_max_results_is_rejected() -> Result<()> {
    let stderr = Run::new()?
        .arg(".")
        .arg("--max-results")
        .arg("0")
        .failure()?;

    assert!(
        stderr.contains("--max-results"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn a_missing_path_fails_with_a_readable_error() -> Result<()> {
    let stderr = Run::new()?
        .arg("/definitely/not/a/real/directory")
        .failure()?;

    assert!(
        stderr.contains("Path not found"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn a_default_run_prints_nothing_but_projects() -> Result<()> {
    let temp = sample_tree()?;

    let output = Run::new()?.arg(temp.path()).output()?;

    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn verbose_diagnostics_stay_off_stdout() -> Result<()> {
    let temp = sample_tree()?;

    let run = Run::new()?.arg(temp.path()).arg("--verbose");
    let output = run.output()?;

    assert_eq!(run.projects()?, joined(temp.path(), &["alpha", "beta"]));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Searching in"),
        "expected search diagnostics on stderr"
    );
    Ok(())
}

#[test]
fn projects_under_home_print_with_a_tilde() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?.home(temp.path()).arg(temp.path()).projects()?;

    assert_eq!(projects, ["~/alpha", "~/beta"]);
    Ok(())
}

#[test]
fn configured_home_paths_are_expanded() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?
        .home(temp.path())
        .config("search_dirs = [\"~\"]\n")?
        .projects()?;

    assert_eq!(projects, ["~/alpha", "~/beta"]);
    Ok(())
}

#[test]
fn the_configuration_file_replaces_the_built_in_defaults() -> Result<()> {
    let temp = TempDir::new()?;
    let workspace = temp.path().join("workspace");
    write_all(&[
        (&workspace.join("workspace.root"), ""),
        (&workspace.join("member/package.json"), "{}"),
        (
            &temp.path().join("rust/Cargo.toml"),
            "[package]\nname = \"rust\"\n",
        ),
    ])?;

    let projects = Run::new()?
        .config(&format!(
            "search_dirs = [\"{}\"]\nmarker_files = [\"package.json\"]\nworkspace_files = [\"workspace.root\"]\n",
            temp.path().display()
        ))?
        .projects()?;

    assert_eq!(projects, [workspace.display().to_string()]);
    Ok(())
}

#[test]
fn command_line_paths_override_the_configured_ones() -> Result<()> {
    let configured = sample_tree()?;
    let requested = TempDir::new()?;
    repository(&requested.path().join("gamma"))?;

    let projects = Run::new()?
        .config(&format!(
            "search_dirs = [\"{}\"]\n",
            configured.path().display()
        ))?
        .arg(requested.path())
        .projects()?;

    assert_eq!(projects, joined(requested.path(), &["gamma"]));
    Ok(())
}

#[test]
fn an_unknown_configuration_key_is_rejected() -> Result<()> {
    let stderr = Run::new()?.config("dpeth = 3\n")?.failure()?;

    assert!(
        stderr.contains("unknown field"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn a_malformed_configuration_file_is_reported_with_its_path() -> Result<()> {
    let run = Run::new()?.config("depth = [\n")?;
    let stderr = run.failure()?;

    assert!(
        stderr.contains("Failed to parse configuration at") && stderr.contains("config.toml"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

fn write_all(files: &[(&Path, &str)]) -> Result<()> {
    for (path, contents) in files {
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        write(path, contents)?;
    }
    Ok(())
}
