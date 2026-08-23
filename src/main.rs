use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use projfind::{
    config::{Config, contract_tilde, home},
    dependencies::Dependencies,
    finder::ProjectFinder,
};
use std::io::stderr;
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

    let home = home();
    for project in projects {
        println!("{}", contract_tilde(&project, home.as_deref()).display());
    }

    Ok(())
}

/// Diagnostics go to stderr, so stdout stays a clean list of paths for
/// whatever the results are piped into.
fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { Level::INFO } else { Level::ERROR };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_writer(stderr)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
