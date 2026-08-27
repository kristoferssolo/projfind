use std::{
    io,
    path::{Path, PathBuf},
    time::SystemTimeError,
};
use thiserror::Error;
use tokio::{sync::AcquireError, task::JoinError};

#[derive(Debug, Error)]
pub enum ProjectFinderError {
    #[error("Dependency not found: {name}. Install it from {url} and try again.")]
    DependencyNotFound {
        name: &'static str,
        url: &'static str,
    },

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Failed to run `{}`", .binary.display())]
    Command {
        binary: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("`{}` produced no stdout to read", .binary.display())]
    MissingStdout { binary: PathBuf },

    #[error("Failed to read {}", .path.display())]
    ReadFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to parse configuration at {}", .path.display())]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("Failed to parse the built-in configuration")]
    ParseDefaultConfig {
        #[source]
        source: toml::de::Error,
    },

    #[error("Failed to parse project history at {}", .path.display())]
    ParseHistory {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("Project history at {} uses unsupported version {version}", .path.display())]
    UnsupportedHistoryVersion { path: PathBuf, version: u8 },

    #[error("Failed to serialize project history")]
    SerializeHistory {
        #[source]
        source: toml::ser::Error,
    },

    #[error("Failed to write {}", .path.display())]
    WriteFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("The system clock is before the Unix epoch")]
    InvalidSystemTime {
        #[source]
        source: SystemTimeError,
    },

    #[error("Failed to schedule the search of {}", .path.display())]
    Scheduling {
        path: PathBuf,
        #[source]
        source: AcquireError,
    },

    #[error("The search of {} did not finish", .path.display())]
    TaskFailed {
        path: PathBuf,
        #[source]
        source: JoinError,
    },
}

impl ProjectFinderError {
    #[must_use]
    pub fn command(binary: &Path, source: io::Error) -> Self {
        Self::Command {
            binary: binary.to_path_buf(),
            source,
        }
    }

    #[must_use]
    pub fn read_file(path: &Path, source: io::Error) -> Self {
        Self::ReadFile {
            path: path.to_path_buf(),
            source,
        }
    }

    #[must_use]
    pub fn write_file(path: &Path, source: io::Error) -> Self {
        Self::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, ProjectFinderError>;
