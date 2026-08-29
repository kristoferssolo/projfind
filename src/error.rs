//! The crate's error type.
//!
//! Every fallible operation reports through [`Error`], so a failure always
//! names the path or pattern it happened on. The binary turns these into
//! reports at the outermost boundary.

use std::{
    io,
    path::{Path, PathBuf},
    time::SystemTimeError,
};
use thiserror::Error as ThisError;

/// Anything that can go wrong while discovering, ranking, or recording
/// projects.
#[derive(Debug, ThisError)]
pub enum Error {
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

    #[error("Invalid exclusion pattern {pattern:?}: {source}")]
    InvalidExcludePattern {
        pattern: String,
        #[source]
        source: ignore::Error,
    },

    #[error("Failed to compile the exclusion patterns: {source}")]
    InvalidExcludeSet {
        #[source]
        source: ignore::Error,
    },

    #[error("The system clock is before the Unix epoch")]
    InvalidSystemTime {
        #[source]
        source: SystemTimeError,
    },

    #[error("Failed to write project output")]
    WriteOutput(#[from] io::Error),

    #[error("Failed to serialize project output as JSON")]
    SerializeJson(#[from] serde_json::Error),
}

impl Error {
    /// Reports a failure to read `path`.
    #[must_use]
    pub fn read_file(path: &Path, source: io::Error) -> Self {
        Self::ReadFile {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Reports a failure to write `path`.
    #[must_use]
    pub fn write_file(path: &Path, source: io::Error) -> Self {
        Self::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// A [`Result`](std::result::Result) that fails with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
