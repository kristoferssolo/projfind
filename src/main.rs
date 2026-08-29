use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use mekle::{
    completions,
    config::{Config, HistoryCommand, Invocation, cli_command},
    error::Error,
    finder::{ProjectFinder, root::RootResolver},
    history::{History, HistoryEntry, ScoreChange},
    output::{rank_projects, write_entries, write_projects},
    paths,
};
use std::{
    io::{stderr, stdout},
    path::Path,
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;

    match Invocation::load().wrap_err("Failed to load arguments")? {
        Invocation::Find(config) => find_projects(config),
        Invocation::Completions(shell) => {
            completions::generate(shell, &mut cli_command(), &mut stdout())?;
            Ok(())
        }
        Invocation::Init(shell) => {
            print!("{}", shell.init());
            Ok(())
        }
        Invocation::Add { path, config } => add_project(&path, &RootResolver::from_config(&config)),
        Invocation::History(command) => manage_history(command),
    }
}

fn find_projects(config: Config) -> Result<()> {
    init_logging(config.verbose).wrap_err("Failed to set up logging")?;

    // Ranking needs the complete result set, so the limit is applied last.
    let max_results = config.max_results;
    let output = config.output;
    let projects = ProjectFinder::new(config)
        .find_project_details()
        .wrap_err("Failed to find projects")?;

    let mut projects = rank_projects(projects, &open_history()?.entries()?);
    if let Some(max) = max_results {
        projects.truncate(max.get());
    }

    let stdout = stdout();
    write_projects(
        &mut stdout.lock(),
        &projects,
        output,
        paths::home().as_deref(),
    )?;
    Ok(())
}

fn add_project(path: &Path, resolver: &RootResolver) -> Result<()> {
    if !path.is_dir() {
        return Err(Error::PathNotFound(path.to_path_buf()).into());
    }

    let project = resolver.resolve_directory(&paths::normalize(path)?)?;
    open_history()?.record(&project)?;
    Ok(())
}

fn manage_history(command: HistoryCommand) -> Result<()> {
    let mut history = open_history()?;
    match command {
        HistoryCommand::List => print_entries(history.entries()?)?,
        HistoryCommand::Show { path } => {
            let path = paths::normalize(&path)?;
            let entry = history
                .entries()?
                .into_iter()
                .find(|entry| entry.path == path)
                .ok_or(Error::HistoryEntryNotFound(path))?;
            print_entries([entry])?;
        }
        HistoryCommand::Set { path, score } => {
            history.update(&paths::normalize(&path)?, ScoreChange::Set(score))?;
        }
        HistoryCommand::Adjust { path, delta } => {
            history.update(&paths::normalize(&path)?, ScoreChange::Adjust(delta))?;
        }
        HistoryCommand::Remove { path } => {
            history.update(&paths::normalize(&path)?, ScoreChange::Remove)?;
        }
        HistoryCommand::Prune => history.prune()?,
        HistoryCommand::Clear => history.clear()?,
    }
    Ok(())
}

fn open_history() -> Result<History> {
    let path = paths::history_file().ok_or(Error::HistoryLocationNotFound)?;
    Ok(History::open(path)?)
}

fn print_entries(entries: impl IntoIterator<Item = HistoryEntry>) -> Result<()> {
    let stdout = stdout();
    write_entries(&mut stdout.lock(), entries, paths::home().as_deref())?;
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
