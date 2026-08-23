use clap::Parser;
use std::{num::NonZeroUsize, path::PathBuf};

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version,
    about = "Find coding projects in specified directories"
)]
pub struct Config {
    /// Directories to search for projects
    #[arg(default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Maximum search depth
    #[arg(short, long, default_value_t = 5)]
    pub depth: usize,

    /// Show verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Maximum number of results to return [default: unlimited]
    #[arg(short = 'n', long)]
    pub max_results: Option<NonZeroUsize>,
}
