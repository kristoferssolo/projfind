use crate::errors::{ProjectFinderError, Result};
use std::path::PathBuf;
use tracing::info;
use which::which;

const FD_BINARIES: [&str; 2] = ["fd", "fdfind"];

#[derive(Debug, Clone)]
pub struct Dependencies {
    pub fd_path: PathBuf,
}

impl Dependencies {
    pub fn new(fd_path: impl Into<PathBuf>) -> Self {
        Self {
            fd_path: fd_path.into(),
        }
    }

    /// Finds `fd` or `fdfind` on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns an error when neither binary is available.
    pub fn check() -> Result<Self> {
        info!("Checking dependencies...");

        FD_BINARIES
            .iter()
            .find_map(|binary| {
                let path = which(binary).ok()?;
                info!("Found {binary} at: {}", path.display());
                Some(Self::new(path))
            })
            .ok_or(ProjectFinderError::DependencyNotFound {
                name: "fd (or fdfind)",
                url: "https://github.com/sharkdp/fd",
            })
    }
}
