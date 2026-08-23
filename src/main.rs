mod commands;
mod config;
mod dependencies;
mod errors;
mod finder;
mod marker;

use crate::{config::Config, dependencies::Dependencies, finder::ProjectFinder};
use clap::Parser;
use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // A missing `fd` or an unreadable path is a user error, not a crash, so
    // keep the report to the error chain itself.
    HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;

    let config = Config::parse();
    init_logging(config.verbose).wrap_err("Failed to set up logging")?;

    let deps = Dependencies::check()?;
    let projects = ProjectFinder::new(config, deps)
        .find_projects()
        .await
        .wrap_err("Failed to find projects")?;

    for project in projects {
        println!("{}", project.display());
    }

    Ok(())
}

/// Send `info` and above to stderr when verbose, otherwise errors only.
fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { Level::INFO } else { Level::ERROR };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
