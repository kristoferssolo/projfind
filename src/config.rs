use crate::completions::CompletionShell;
use crate::errors::{ProjectFinderError, Result};
use clap::{CommandFactory, Parser, Subcommand};
use serde::Deserialize;
use std::{
    env,
    ffi::OsString,
    fs,
    io::ErrorKind,
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

#[derive(Debug)]
pub enum Invocation {
    Find(Config),
    Completions(CompletionShell),
    Init(CompletionShell),
    Add(PathBuf, Config),
    History(HistoryCommand),
}

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
        let cli = Cli::parse();
        let home = home();
        match &cli.command {
            Some(CliCommand::Completions { shell }) => return Ok(Self::Completions(*shell)),
            Some(CliCommand::Init { shell }) => return Ok(Self::Init(*shell)),
            Some(CliCommand::Add { path }) => {
                let path = expand_tilde(path, home.as_deref());
                let config = Config::from_sources(cli, config_file_path().as_deref())?;
                return Ok(Self::Add(path, config));
            }
            Some(CliCommand::History { command }) => {
                return Ok(Self::History(command.clone().expand_path(home.as_deref())));
            }
            None => {}
        }

        Config::from_sources(cli, config_file_path().as_deref()).map(Self::Find)
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
                path: expand_tilde(&path, home),
            },
            Self::Set { path, score } => Self::Set {
                path: expand_tilde(&path, home),
                score,
            },
            Self::Adjust { path, delta } => Self::Adjust {
                path: expand_tilde(&path, home),
                delta,
            },
            Self::Remove { path } => Self::Remove {
                path: expand_tilde(&path, home),
            },
            command @ (Self::List | Self::Prune | Self::Clear) => command,
        }
    }
}

impl Config {
    /// Loads the effective configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration file cannot be read or parsed.
    pub fn load() -> Result<Self> {
        Self::from_sources(Cli::parse(), config_file_path().as_deref())
    }

    /// Returns the embedded defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded TOML is invalid.
    pub fn defaults() -> Result<Self> {
        toml::from_str(DEFAULT_CONFIG)
            .map_err(|source| ProjectFinderError::ParseDefaultConfig { source })
    }

    fn from_sources(cli: Cli, path: Option<&Path>) -> Result<Self> {
        let mut config = Self::defaults()?;

        if let Some(path) = path
            && let Some(file) = read_config_file(path)?
        {
            file.apply_to(&mut config);
        }

        cli.apply_to(&mut config);
        config.expand_paths(home().as_deref());
        Ok(config)
    }

    fn expand_paths(&mut self, home: Option<&Path>) {
        for path in &mut self.paths {
            *path = expand_tilde(path, home);
        }
    }
}

// `~user` is intentionally left unchanged.
fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let (Some(home), Ok(rest)) = (home, path.strip_prefix("~")) else {
        return path.to_path_buf();
    };

    home.join(rest)
}

#[must_use]
pub fn contract_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return path.to_path_buf();
    };

    if rest.as_os_str().is_empty() {
        return PathBuf::from("~");
    }

    Path::new("~").join(rest)
}

impl FileConfig {
    fn apply_to(self, config: &mut Config) {
        if let Some(search_dirs) = self.search_dirs {
            config.paths = search_dirs;
        }
        if let Some(marker_files) = self.marker_files {
            config.marker_files = marker_files;
        }
        if let Some(workspace_files) = self.workspace_files {
            config.workspace_files = workspace_files;
        }
        if let Some(exclude) = self.exclude {
            config.exclude = exclude;
        }
        if let Some(depth) = self.depth {
            config.depth = depth;
        }
        if let Some(verbose) = self.verbose {
            config.verbose = verbose;
        }
        if let Some(max_results) = self.max_results {
            config.max_results = Some(max_results);
        }
    }
}

impl Cli {
    fn apply_to(self, config: &mut Config) {
        if !self.paths.is_empty() {
            config.paths = self.paths;
        }
        if let Some(depth) = self.depth {
            config.depth = depth;
        }
        if self.verbose {
            config.verbose = true;
        }
        if let Some(max_results) = self.max_results {
            config.max_results = Some(max_results);
        }
        config.exclude.extend(self.exclude);
        config.output = if self.json {
            OutputFormat::Json
        } else if self.null {
            OutputFormat::Null
        } else {
            OutputFormat::Path
        };
    }
}

fn read_config_file(path: &Path) -> Result<Option<FileConfig>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ProjectFinderError::read_file(path, error)),
    };
    let config = toml::from_str(&contents).map_err(|source| ProjectFinderError::ParseConfig {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(Some(config))
}

pub fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn config_file_path() -> Option<PathBuf> {
    config_file_path_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

fn config_file_path_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let config_home = xdg_config_home
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|path| path.join(".config"))
        })?;

    Some(config_home.join("mekle/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some};
    use rstest::rstest;
    use std::fs::write;
    use tempfile::TempDir;

    #[test]
    fn defaults_are_used_without_a_config_file() -> color_eyre::Result<()> {
        let cli = Cli::try_parse_from(["mekle"])?;
        let config = Config::from_sources(cli, None)?;

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
    fn file_values_replace_built_in_defaults() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("config.toml");
        write(
            &path,
            r#"
search_dirs = ["/projects"]
marker_files = ["project.toml"]
workspace_files = ["workspace.toml"]
depth = 12
verbose = true
max_results = 20
"#,
        )?;
        let cli = Cli::try_parse_from(["mekle"])?;

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(config.paths, [PathBuf::from("/projects")]);
        assert_eq!(config.marker_files, ["project.toml"]);
        assert_eq!(config.workspace_files, ["workspace.toml"]);
        assert_eq!(config.depth, 12);
        assert!(config.verbose);
        assert_eq!(config.max_results.map(NonZeroUsize::get), Some(20));
        Ok(())
    }

    #[test]
    fn command_line_values_override_the_file() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("config.toml");
        write(
            &path,
            "search_dirs = [\"/from-file\"]\ndepth = 12\nmax_results = 20\n",
        )?;
        let cli =
            Cli::try_parse_from(["mekle", "--depth", "3", "--max-results", "2", "/from-cli"])?;

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(config.paths, [PathBuf::from("/from-cli")]);
        assert_eq!(config.depth, 3);
        assert_eq!(config.max_results.map(NonZeroUsize::get), Some(2));
        Ok(())
    }

    #[test]
    fn missing_config_file_is_ignored() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let cli = Cli::try_parse_from(["mekle"])?;

        assert_ok!(Config::from_sources(
            cli,
            Some(&temp.path().join("missing.toml"))
        ));
        Ok(())
    }

    #[test]
    fn xdg_config_home_takes_precedence() {
        let path = config_file_path_from(Some("/xdg".into()), Some("/home/user".into()));

        assert_eq!(path, Some(PathBuf::from("/xdg/mekle/config.toml")));
    }

    #[test]
    fn home_is_the_fallback_config_location() {
        let path = config_file_path_from(None, Some("/home/user".into()));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/user/.config/mekle/config.toml"))
        );
    }

    #[test]
    fn no_config_location_is_valid() {
        assert_none!(config_file_path_from(None, None));
    }

    #[rstest]
    #[case(
        "~/repos",
        "/home/user/repos",
        "a leading tilde becomes the home directory"
    )]
    #[case("~", "/home/user", "a bare tilde is the home directory itself")]
    #[case("/tmp/~/x", "/tmp/~/x", "a tilde below the root is an ordinary name")]
    #[case("~user/repos", "~user/repos", "the ~user form is left to the shell")]
    #[case("./repos", "./repos", "paths without a tilde are untouched")]
    fn tilde_expansion(#[case] input: &str, #[case] expected: &str, #[case] reason: &str) {
        let home = PathBuf::from("/home/user");

        assert_eq!(
            expand_tilde(Path::new(input), Some(&home)),
            PathBuf::from(expected),
            "{reason}"
        );
    }

    #[rstest]
    #[case("/home/user/repos", "~/repos", "a path under home is shortened")]
    #[case("/home/user", "~", "home itself is a bare tilde")]
    #[case("/mnt/data/repos", "/mnt/data/repos", "paths elsewhere are untouched")]
    #[case(
        "/home/username/repos",
        "/home/username/repos",
        "a longer sibling name is not a prefix match"
    )]
    fn tilde_contraction(#[case] input: &str, #[case] expected: &str, #[case] reason: &str) {
        let home = PathBuf::from("/home/user");

        assert_eq!(
            contract_tilde(Path::new(input), Some(&home)),
            PathBuf::from(expected),
            "{reason}"
        );
    }

    #[rstest]
    #[case("~/repos")]
    #[case("~")]
    #[case("/mnt/data")]
    fn contraction_undoes_expansion(#[case] path: &str) {
        let home = PathBuf::from("/home/user");
        let expanded = expand_tilde(Path::new(path), Some(&home));

        assert_eq!(contract_tilde(&expanded, Some(&home)), PathBuf::from(path));
    }

    #[test]
    fn tildes_survive_a_missing_home() {
        assert_eq!(
            expand_tilde(Path::new("~/repos"), None),
            PathBuf::from("~/repos")
        );
    }

    #[test]
    fn configured_search_dirs_are_expanded() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("config.toml");
        write(&path, "search_dirs = [\"~/repos\", \"/absolute\"]\n")?;
        let cli = Cli::try_parse_from(["mekle"])?;
        let home = assert_some!(home());

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(
            config.paths,
            [home.join("repos"), PathBuf::from("/absolute")]
        );
        Ok(())
    }

    #[test]
    fn exclusions_are_loaded_from_the_file() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("config.toml");
        write(
            &path,
            "exclude = [\"target/\", \"**/vendor/\", \"/archive/\"]\n",
        )?;
        let cli = Cli::try_parse_from(["mekle"])?;

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(config.exclude, ["target/", "**/vendor/", "/archive/"]);
        Ok(())
    }

    #[test]
    fn repeated_cli_exclusions_are_collected() -> color_eyre::Result<()> {
        let cli = Cli::try_parse_from([
            "mekle",
            "--exclude",
            "target/",
            "--exclude",
            "node_modules/",
        ])?;

        let config = Config::from_sources(cli, None)?;

        assert_eq!(config.exclude, ["target/", "node_modules/"]);
        Ok(())
    }

    #[test]
    fn cli_exclusions_append_to_configured_ones() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("config.toml");
        write(&path, "exclude = [\"target/\"]\n")?;
        let cli = Cli::try_parse_from(["mekle", "--exclude", "dist/"])?;

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(config.exclude, ["target/", "dist/"]);
        Ok(())
    }
}
