use std::{
    io,
    path::{Path, PathBuf},
    time::SystemTimeError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectFinderError {
    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Failed to resolve project path {}", .path.display())]
    ResolvePath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Could not determine the project history location")]
    HistoryLocationNotFound,

    #[error("Project is not present in history: {}", .0.display())]
    HistoryEntryNotFound(PathBuf),

    #[error("Project score must be a finite number of at least 1, got {0}")]
    InvalidScore(f64),

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
}

impl ProjectFinderError {
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
