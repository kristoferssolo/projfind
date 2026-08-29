//! Where mekle's own files live, and how paths are shown to a user.

use crate::error::{Error, Result};
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The directory mekle owns inside a base directory.
const APP_DIR: &str = "mekle";

/// The user's home directory, if the environment names one.
#[must_use]
pub fn home() -> Option<PathBuf> {
    non_empty(env::var_os("HOME"))
}

/// The configuration file location, from `XDG_CONFIG_HOME` or `~/.config`.
#[must_use]
pub fn config_file() -> Option<PathBuf> {
    app_file(
        env::var_os("XDG_CONFIG_HOME"),
        env::var_os("HOME"),
        ".config",
        "config.toml",
    )
}

/// The history file location, from `XDG_DATA_HOME` or `~/.local/share`.
#[must_use]
pub fn history_file() -> Option<PathBuf> {
    app_file(
        env::var_os("XDG_DATA_HOME"),
        env::var_os("HOME"),
        ".local/share",
        "history.toml",
    )
}

/// Resolves one of mekle's files under `xdg_base`, falling back to
/// `home/home_relative`.
fn app_file(
    xdg_base: Option<OsString>,
    home: Option<OsString>,
    home_relative: &str,
    file: &str,
) -> Option<PathBuf> {
    let base =
        non_empty(xdg_base).or_else(|| non_empty(home).map(|home| home.join(home_relative)))?;

    Some(base.join(APP_DIR).join(file))
}

fn non_empty(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Replaces a leading `~` with `home`.
///
/// `~user` is left alone: only a shell knows how to resolve it.
#[must_use]
pub fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let (Some(home), Ok(rest)) = (home, path.strip_prefix("~")) else {
        return path.to_path_buf();
    };

    home.join(rest)
}

/// Shortens a path under `home` to a leading `~`, undoing [`expand_tilde`].
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

/// Turns `path` into an absolute path, resolving symlinks when it exists.
///
/// History is keyed by resolved paths, so the same project recorded through a
/// symlink and through its real location is one entry.
///
/// # Errors
///
/// Returns an error if `path` cannot be resolved.
pub fn normalize(path: &Path) -> Result<PathBuf> {
    let resolved = if exists_now(path) {
        std::fs::canonicalize(path)
    } else {
        std::path::absolute(path)
    };

    resolved.map_err(|source| Error::ResolvePath {
        path: path.to_path_buf(),
        source,
    })
}

/// A path that cannot be inspected is treated as absent, leaving the caller to
/// fail on the absolute form instead.
fn exists_now(path: &Path) -> bool {
    path.try_exists().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok};
    use rstest::rstest;
    use tempfile::TempDir;

    fn config_file_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
        app_file(xdg, home, ".config", "config.toml")
    }

    fn history_file_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
        app_file(xdg, home, ".local/share", "history.toml")
    }

    #[test]
    fn xdg_config_home_takes_precedence() {
        let path = config_file_from(Some("/xdg".into()), Some("/home/user".into()));

        assert_eq!(path, Some(PathBuf::from("/xdg/mekle/config.toml")));
    }

    #[test]
    fn home_is_the_fallback_config_location() {
        let path = config_file_from(None, Some("/home/user".into()));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/user/.config/mekle/config.toml"))
        );
    }

    #[test]
    fn xdg_data_home_takes_precedence() {
        let path = history_file_from(Some("/xdg".into()), Some("/home/user".into()));

        assert_eq!(path, Some(PathBuf::from("/xdg/mekle/history.toml")));
    }

    #[test]
    fn home_is_the_fallback_history_location() {
        let path = history_file_from(None, Some("/home/user".into()));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/user/.local/share/mekle/history.toml"))
        );
    }

    #[test]
    fn an_empty_variable_is_no_variable() {
        assert_none!(config_file_from(Some(OsString::new()), None));
        assert_none!(config_file_from(None, Some(OsString::new())));
    }

    #[test]
    fn no_config_location_is_valid() {
        assert_none!(config_file_from(None, None));
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
    fn a_missing_path_still_normalizes_to_an_absolute_one() {
        let temp = TempDir::new().expect("create temp dir");
        let absent = temp.path().join("absent");

        assert_ok!(normalize(&absent));
    }
}
