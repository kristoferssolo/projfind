use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use projfind::{
    config::{Config, Invocation, contract_tilde, home},
    dependencies::Dependencies,
    errors::ProjectFinderError,
    finder::ProjectFinder,
    history::{History, history_file_path},
};
use std::{fs, io::stderr, path::Path};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;

    match Invocation::load().wrap_err("Failed to load arguments")? {
        Invocation::Find(config) => find_projects(config).await,
        Invocation::Add(path) => add_project(&path),
    }
}

async fn find_projects(mut config: Config) -> Result<()> {
    init_logging(config.verbose).wrap_err("Failed to set up logging")?;

    // Ranking needs the complete result set before applying the output limit.
    let max_results = config.max_results.take();
    let deps = Dependencies::check()?;
    let mut projects = ProjectFinder::new(config, deps)
        .find_projects()
        .await
        .wrap_err("Failed to find projects")?;
    if let Some(path) = history_file_path() {
        History::open(path)?.sort(&mut projects)?;
    }
    if let Some(max) = max_results {
        projects.truncate(max.get());
    }

    let home = home();
    for project in projects {
        println!("{}", contract_tilde(&project, home.as_deref()).display());
    }

    Ok(())
}

fn add_project(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(ProjectFinderError::PathNotFound(path.to_path_buf()).into());
    }
    let project = fs::canonicalize(path).map_err(|source| ProjectFinderError::ResolvePath {
        path: path.to_path_buf(),
        source,
    })?;
    let history_path = history_file_path().ok_or(ProjectFinderError::HistoryLocationNotFound)?;
    History::open(history_path)?.record(&project)?;
    Ok(())
}

fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { Level::INFO } else { Level::ERROR };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_writer(stderr)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
