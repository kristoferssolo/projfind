//! The command line, the configuration file, and how they layer.
//!
//! Settings come from three sources, each overriding the one before it: the
//! defaults embedded at compile time, the user's configuration file, and the
//! command line.

use crate::{
    completions::CompletionShell,
    error::{Error, Result},
    fs, paths,
};
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Deserialize;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

const DEFAULT_CONFIG: &str = include_str!("../config/config.toml");

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version,
    about = "Find coding projects in specified directories"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,

    #[command(flatten)]
    search: SearchArgs,
}

/// The options that shape a search, shared by the bare invocation and `add`.
#[derive(Debug, Args, Clone)]
struct SearchArgs {
    #[arg(help = "Directories to search")]
    paths: Vec<PathBuf>,

    #[arg(short, long, help = "Maximum search depth")]
    depth: Option<usize>,

    #[arg(short, long, help = "Print search progress")]
    verbose: bool,

    #[arg(short = 'n', long, help = "Maximum number of results")]
    max_results: Option<NonZeroUsize>,

    #[arg(long, conflicts_with = "null", help = "Print newline-delimited JSON")]
    json: bool,

    #[arg(
        short = '0',
        long,
        conflicts_with = "json",
        help = "Print uncontracted paths separated by NUL bytes"
    )]
    null: bool,

    #[arg(
        long,
        value_name = "PATTERN",
        help = "Exclude entries matching a gitignore-style pattern, relative to each search directory"
    )]
    exclude: Vec<String>,
}

#[derive(Debug, Subcommand, Clone)]
enum CliCommand {
    /// Generates shell completions.
    Completions { shell: CompletionShell },
    /// Generates a shell integration that defines `m`.
    Init { shell: CompletionShell },
    /// Records a visit to a project directory.
    Add {
        #[arg(help = "Project directory to record")]
        path: PathBuf,
    },

    /// Inspects or changes project history.
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
}

/// What the user asked mekle to do.
#[derive(Debug)]
pub enum Invocation {
    Find(Config),
    Completions(CompletionShell),
    Init(CompletionShell),
    Add { path: PathBuf, config: Config },
    History(HistoryCommand),
}

/// How discovered projects are printed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Path,
    Json,
    Null,
}

#[derive(Debug, Subcommand, Clone)]
pub enum HistoryCommand {
    /// Lists every recorded project.
    List,
    /// Shows one recorded project.
    Show {
        #[arg(help = "Project directory to inspect")]
        path: PathBuf,
    },
    /// Sets a project's raw score.
    Set {
        #[arg(help = "Project directory to change")]
        path: PathBuf,
        #[arg(help = "New raw score")]
        score: f64,
    },
    /// Adds a positive or negative amount to a project's raw score.
    Adjust {
        #[arg(help = "Project directory to change")]
        path: PathBuf,
        #[arg(allow_hyphen_values = true, help = "Amount to add to the raw score")]
        delta: f64,
    },
    /// Removes one project from history.
    Remove {
        #[arg(help = "Project directory to remove")]
        path: PathBuf,
    },
    /// Removes projects whose paths no longer exist.
    Prune,
    /// Removes every project from history.
    Clear,
}

/// The configuration file, where every setting is optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    search_dirs: Option<Vec<PathBuf>>,
    marker_files: Option<Vec<String>>,
    workspace_files: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    depth: Option<usize>,
    verbose: Option<bool>,
    max_results: Option<NonZeroUsize>,
}

/// The effective settings for one run.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(rename = "search_dirs")]
    pub paths: Vec<PathBuf>,
    pub marker_files: Vec<String>,
    pub workspace_files: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub depth: usize,
    pub verbose: bool,
    #[serde(default)]
    pub max_results: Option<NonZeroUsize>,
    #[serde(skip)]
    pub output: OutputFormat,
}

impl Invocation {
    /// Loads the requested command and its effective configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration file cannot be read or parsed.
    pub fn load() -> Result<Self> {
        Self::from_cli(Cli::parse(), paths::config_file().as_deref())
    }

    fn from_cli(cli: Cli, config_path: Option<&Path>) -> Result<Self> {
        let home = paths::home();
        let Cli { command, search } = cli;

        match command {
            Some(CliCommand::Completions { shell }) => Ok(Self::Completions(shell)),
            Some(CliCommand::Init { shell }) => Ok(Self::Init(shell)),
            Some(CliCommand::Add { path }) => Ok(Self::Add {
                path: paths::expand_tilde(&path, home.as_deref()),
                config: Config::from_sources(search, config_path)?,
            }),
            Some(CliCommand::History { command }) => {
                Ok(Self::History(command.expand_path(home.as_deref())))
            }
            None => Config::from_sources(search, config_path).map(Self::Find),
        }
    }
}

/// Builds the complete command definition.
#[must_use]
pub fn cli_command() -> clap::Command {
    Cli::command()
}

impl HistoryCommand {
    fn expand_path(self, home: Option<&Path>) -> Self {
        match self {
            Self::Show { path } => Self::Show {
                path: paths::expand_tilde(&path, home),
            },
            Self::Set { path, score } => Self::Set {
                path: paths::expand_tilde(&path, home),
                score,
            },
            Self::Adjust { path, delta } => Self::Adjust {
                path: paths::expand_tilde(&path, home),
                delta,
            },
            Self::Remove { path } => Self::Remove {
                path: paths::expand_tilde(&path, home),
            },
            command @ (Self::List | Self::Prune | Self::Clear) => command,
        }
    }
}

impl Config {
    /// Returns the embedded defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded TOML is invalid.
    pub fn defaults() -> Result<Self> {
        toml::from_str(DEFAULT_CONFIG).map_err(|source| Error::ParseDefaultConfig { source })
    }

    fn from_sources(cli: SearchArgs, path: Option<&Path>) -> Result<Self> {
        let mut config = Self::defaults()?;

        if let Some(file) = path.map(read_config_file).transpose()?.flatten() {
            file.apply_to(&mut config);
        }

        cli.apply_to(&mut config);
        let home = paths::home();
        for path in &mut config.paths {
            *path = paths::expand_tilde(path, home.as_deref());
        }

        Ok(config)
    }
}

/// Overwrites `target` when the layer above supplied a value.
fn overlay<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

impl FileConfig {
    fn apply_to(self, config: &mut Config) {
        overlay(&mut config.paths, self.search_dirs);
        overlay(&mut config.marker_files, self.marker_files);
        overlay(&mut config.workspace_files, self.workspace_files);
        overlay(&mut config.exclude, self.exclude);
        overlay(&mut config.depth, self.depth);
        overlay(&mut config.verbose, self.verbose);
        config.max_results = self.max_results.or(config.max_results);
    }
}

impl SearchArgs {
    fn apply_to(self, config: &mut Config) {
        config.output = self.output_format();
        config.max_results = self.max_results.or(config.max_results);
        overlay(&mut config.depth, self.depth);
        if self.verbose {
            config.verbose = true;
        }
        if !self.paths.is_empty() {
            config.paths = self.paths;
        }
        // Exclusions accumulate: the command line narrows a configured search
        // rather than replacing it.
        config.exclude.extend(self.exclude);
    }

    const fn output_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else if self.null {
            OutputFormat::Null
        } else {
            OutputFormat::Path
        }
    }
}

fn read_config_file(path: &Path) -> Result<Option<FileConfig>> {
    fs::read(path)?
        .map(|contents| {
            toml::from_str(&contents).map_err(|source| Error::ParseConfig {
                path: path.to_path_buf(),
                source,
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some};
    use std::fs::write;
    use tempfile::TempDir;

    /// Parses `args` and layers it over `path`, the way a run would.
    fn config_from(args: &[&str], path: Option<&Path>) -> Result<Config> {
        let cli = Cli::try_parse_from(args).expect("the arguments parse");
        Config::from_sources(cli.search, path)
    }

    /// Writes a configuration file holding `contents` and returns its path.
    fn config_file(temp: &TempDir, contents: &str) -> PathBuf {
        let path = temp.path().join("config.toml");
        write(&path, contents).expect("write the configuration file");
        path
    }

    #[test]
    fn defaults_are_used_without_a_config_file() -> Result<()> {
        let config = config_from(&["mekle"], None)?;

        assert_eq!(config.paths, [PathBuf::from(".")]);
        assert_eq!(config.depth, 5);
        assert!(config.exclude.is_empty());
        assert!(config.marker_files.iter().any(|file| file == "Cargo.toml"));
        assert!(
            config
                .workspace_files
                .iter()
                .any(|file| file == "pnpm-workspace.yaml")
        );
        assert_none!(config.max_results);
        assert_eq!(config.output, OutputFormat::Path);
        Ok(())
    }

    #[test]
    fn file_values_replace_built_in_defaults() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let path = config_file(
            &temp,
            r#"
search_dirs = ["/projects"]
marker_files = ["project.toml"]
workspace_files = ["workspace.toml"]
depth = 12
verbose = true
max_results = 20
"#,
        );

        let config = config_from(&["mekle"], Some(&path))?;

        assert_eq!(config.paths, [PathBuf::from("/projects")]);
        assert_eq!(config.marker_files, ["project.toml"]);
        assert_eq!(config.workspace_files, ["workspace.toml"]);
        assert_eq!(config.depth, 12);
        assert!(config.verbose);
        assert_eq!(config.max_results.map(NonZeroUsize::get), Some(20));
        Ok(())
    }

    #[test]
    fn command_line_values_override_the_file() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let path = config_file(
            &temp,
            "search_dirs = [\"/from-file\"]\ndepth = 12\nmax_results = 20\n",
        );

        let config = config_from(
            &["mekle", "--depth", "3", "--max-results", "2", "/from-cli"],
            Some(&path),
        )?;

        assert_eq!(config.paths, [PathBuf::from("/from-cli")]);
        assert_eq!(config.depth, 3);
        assert_eq!(config.max_results.map(NonZeroUsize::get), Some(2));
        Ok(())
    }

    #[test]
    fn missing_config_file_is_ignored() {
        let temp = TempDir::new().expect("create temp dir");

        assert_ok!(config_from(
            &["mekle"],
            Some(&temp.path().join("missing.toml"))
        ));
    }

    #[test]
    fn configured_search_dirs_are_expanded() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let path = config_file(&temp, "search_dirs = [\"~/repos\", \"/absolute\"]\n");
        let home = assert_some!(paths::home());

        let config = config_from(&["mekle"], Some(&path))?;

        assert_eq!(
            config.paths,
            [home.join("repos"), PathBuf::from("/absolute")]
        );
        Ok(())
    }

    #[test]
    fn exclusions_are_loaded_from_the_file() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let path = config_file(
            &temp,
            "exclude = [\"target/\", \"**/vendor/\", \"/archive/\"]\n",
        );

        let config = config_from(&["mekle"], Some(&path))?;

        assert_eq!(config.exclude, ["target/", "**/vendor/", "/archive/"]);
        Ok(())
    }

    #[test]
    fn repeated_cli_exclusions_are_collected() -> Result<()> {
        let config = config_from(
            &[
                "mekle",
                "--exclude",
                "target/",
                "--exclude",
                "node_modules/",
            ],
            None,
        )?;

        assert_eq!(config.exclude, ["target/", "node_modules/"]);
        Ok(())
    }

    #[test]
    fn cli_exclusions_append_to_configured_ones() -> Result<()> {
        let temp = TempDir::new().expect("create temp dir");
        let path = config_file(&temp, "exclude = [\"target/\"]\n");

        let config = config_from(&["mekle", "--exclude", "dist/"], Some(&path))?;

        assert_eq!(config.exclude, ["target/", "dist/"]);
        Ok(())
    }

    #[test]
    fn add_carries_the_search_configuration() -> Result<()> {
        let cli = Cli::try_parse_from(["mekle", "add", "/projects/one"]).expect("arguments parse");

        let invocation = Invocation::from_cli(cli, None)?;

        match invocation {
            Invocation::Add { path, config } => {
                assert_eq!(path, PathBuf::from("/projects/one"));
                assert_eq!(config.depth, 5);
            }
            other => panic!("expected an add invocation, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn a_bare_invocation_searches() -> Result<()> {
        let cli = Cli::try_parse_from(["mekle", "--json"]).expect("arguments parse");

        match Invocation::from_cli(cli, None)? {
            Invocation::Find(config) => assert_eq!(config.output, OutputFormat::Json),
            other => panic!("expected a find invocation, got {other:?}"),
        }
        Ok(())
    }
}
