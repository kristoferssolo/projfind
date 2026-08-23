mod config;
mod dependencies;
mod errors;
mod finder;
mod scan;

use crate::{config::Config, dependencies::Dependencies, finder::ProjectFinder};
use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;

    let config = Config::load().wrap_err("Failed to load configuration")?;
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

fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { Level::INFO } else { Level::ERROR };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
