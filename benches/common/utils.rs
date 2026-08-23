use color_eyre::eyre::{Result, eyre};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

use super::setup::BenchParams;

pub const BASE_DIR: &str = env!("CARGO_MANIFEST_DIR");

pub fn run_binary_with_args(path: &Path, params: &BenchParams) -> Result<()> {
    let binary_path = PathBuf::from(BASE_DIR).join("target/release/projfind");

    if !binary_path.exists() {
        return Err(eyre!(
            "Binary not found at {}. Did you run `cargo build --release`?",
            binary_path.display()
        ));
    }

    let mut cmd = Command::new(&binary_path);

    cmd.arg(path);

    if let Some(depth) = params.depth {
        cmd.arg("--depth").arg(depth.to_string());
    }

    if let Some(max_results) = params.max_results {
        cmd.arg("--max-results").arg(max_results.to_string());
    }

    if params.verbose {
        cmd.arg("--verbose");
    }

    let output = cmd
        .output()
        .map_err(|e| eyre!("Failed to execute binary {}: {}", binary_path.display(), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "Process failed with status: {}\nStderr: {}",
            output.status,
            stderr
        ));
    }

    Ok(())
}

#[allow(dead_code)]
pub fn create_deep_directory(_base: &Path, _depth: usize) -> Result<()> {
    todo!()
}

#[allow(dead_code)]
pub fn create_wide_directory(_base: &Path, _width: usize) -> Result<()> {
    todo!()
}
