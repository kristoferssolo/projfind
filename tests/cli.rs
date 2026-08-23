//! The binary's own surface: arguments, configuration files, what lands on
//! stdout versus stderr, and what a failure looks like.
//!
//! Which directories count as projects is covered in `discovery.rs`; a case
//! belongs here only if it needs the process.

use color_eyre::eyre::{Result, bail};
use std::{
    ffi::{OsStr, OsString},
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_projfind");

/// One invocation of the binary, with the developer's own environment kept out
/// of it: a real `~/.config/projfind/config.toml` would otherwise decide what
/// these tests search for.
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

    /// Writes `contents` where this run will look for its configuration.
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
            .env("XDG_CONFIG_HOME", self.config_home.path());

        match &self.home {
            Some(home) => command.env("HOME", home),
            None => command.env_remove("HOME"),
        };

        Ok(command.output()?)
    }

    /// The printed projects, after asserting the run succeeded.
    fn projects(&self) -> Result<Vec<String>> {
        let output = self.output()?;
        if !output.status.success() {
            bail!(
                "binary failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8(output.stdout)?
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// The stderr of a run that was expected to fail.
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
}

/// Two projects, named so that sorted output is `alpha` before `beta`.
fn sample_tree() -> Result<TempDir> {
    let temp = TempDir::new()?;
    create_dir_all(temp.path().join("alpha/.git"))?;
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

/// A count of zero would ask for an empty answer, which is never what anyone
/// means, so `clap` rejects it before the search starts.
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

/// Nothing is printed by default beyond the results, so the output pipes
/// straight into a picker.
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

/// `--verbose` has to keep its diagnostics off stdout, or it would break the
/// pipe it exists to help debug.
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

/// Only a shell expands `~`, so the binary contracts it back itself when
/// printing, and the result still has to be a path that resolves.
#[test]
fn projects_under_home_print_with_a_tilde() -> Result<()> {
    let temp = sample_tree()?;

    let projects = Run::new()?.home(temp.path()).arg(temp.path()).projects()?;

    assert_eq!(projects, ["~/alpha", "~/beta"]);
    Ok(())
}

/// A configured `~` is expanded on the way in, which is the half of the round
/// trip a shell would otherwise have done.
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
        // A default marker, to prove the configured list replaced the shipped
        // one rather than adding to it.
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
    create_dir_all(requested.path().join("gamma/.git"))?;

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

/// A typo in a key would otherwise be silently ignored, leaving the setting
/// looking applied.
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
