use crate::errors::{ProjectFinderError, Result};
use clap::Parser;
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
    /// Directories to search for projects [default: configuration or `.`]
    paths: Vec<PathBuf>,

    /// Maximum search depth [default: configuration or 5]
    #[arg(short, long)]
    depth: Option<usize>,

    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Maximum number of results [default: configuration or unlimited]
    #[arg(short = 'n', long)]
    max_results: Option<NonZeroUsize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    search_dirs: Option<Vec<PathBuf>>,
    marker_files: Option<Vec<String>>,
    workspace_files: Option<Vec<String>>,
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
    pub depth: usize,
    pub verbose: bool,
    #[serde(default)]
    pub max_results: Option<NonZeroUsize>,
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::from_sources(Cli::parse(), config_file_path().as_deref())
    }

    fn from_sources(cli: Cli, path: Option<&Path>) -> Result<Self> {
        let mut config = toml::from_str(DEFAULT_CONFIG)
            .map_err(|source| ProjectFinderError::ParseDefaultConfig { source })?;

        if let Some(path) = path
            && let Some(file) = read_config_file(path)?
        {
            file.apply_to(&mut config);
        }

        cli.apply_to(&mut config);
        Ok(config)
    }
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

    Some(config_home.join("project-finder/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok};
    use std::fs::write;
    use tempfile::TempDir;

    #[test]
    fn defaults_are_used_without_a_config_file() -> color_eyre::Result<()> {
        let cli = Cli::try_parse_from(["project-finder"])?;
        let config = Config::from_sources(cli, None)?;

        assert_eq!(config.paths, [PathBuf::from(".")]);
        assert_eq!(config.depth, 5);
        assert!(config.marker_files.iter().any(|file| file == "Cargo.toml"));
        assert!(
            config
                .workspace_files
                .iter()
                .any(|file| file == "pnpm-workspace.yaml")
        );
        assert_none!(config.max_results);
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
        let cli = Cli::try_parse_from(["project-finder"])?;

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
        let cli = Cli::try_parse_from([
            "project-finder",
            "--depth",
            "3",
            "--max-results",
            "2",
            "/from-cli",
        ])?;

        let config = Config::from_sources(cli, Some(&path))?;

        assert_eq!(config.paths, [PathBuf::from("/from-cli")]);
        assert_eq!(config.depth, 3);
        assert_eq!(config.max_results.map(NonZeroUsize::get), Some(2));
        Ok(())
    }

    #[test]
    fn missing_config_file_is_ignored() -> color_eyre::Result<()> {
        let temp = TempDir::new()?;
        let cli = Cli::try_parse_from(["project-finder"])?;

        assert_ok!(Config::from_sources(
            cli,
            Some(&temp.path().join("missing.toml"))
        ));
        Ok(())
    }

    #[test]
    fn xdg_config_home_takes_precedence() {
        let path = config_file_path_from(Some("/xdg".into()), Some("/home/user".into()));

        assert_eq!(path, Some(PathBuf::from("/xdg/project-finder/config.toml")));
    }

    #[test]
    fn home_is_the_fallback_config_location() {
        let path = config_file_path_from(None, Some("/home/user".into()));

        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/user/.config/project-finder/config.toml"
            ))
        );
    }

    #[test]
    fn no_config_location_is_valid() {
        assert_none!(config_file_path_from(None, None));
    }
}
