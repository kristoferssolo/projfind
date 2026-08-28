use color_eyre::{
    config::HookBuilder,
    eyre::{Result, WrapErr},
};
use mekle::{
    completions,
    config::{Config, HistoryCommand, Invocation, cli_command, contract_tilde, home},
    errors::ProjectFinderError,
    finder::{ProjectFinder, root::RootResolver},
    history::{History, HistoryEntry, ScoreChange, history_file_path},
    output::{rank_projects, write_projects},
};
use std::{
    fs,
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
        Invocation::Init(completion_shell) => {
            print!("{}", completion_shell.init());
            Ok(())
        }
        Invocation::Add(path, config) => add_project(&path, &RootResolver::from_config(&config)),
        Invocation::History(command) => manage_history(command),
    }
}

fn manage_history(command: HistoryCommand) -> Result<()> {
    let history_path = history_file_path().ok_or(ProjectFinderError::HistoryLocationNotFound)?;
    let mut history = History::open(history_path)?;
    match command {
        HistoryCommand::List => print_entries(history.entries()?),
        HistoryCommand::Show { path } => {
            let path = normalize_path(&path)?;
            let entry = history
                .entries()?
                .into_iter()
                .find(|entry| entry.path == path)
                .ok_or(ProjectFinderError::HistoryEntryNotFound(path))?;
            print_entries([entry]);
        }
        HistoryCommand::Set { path, score } => {
            history.update(&normalize_path(&path)?, ScoreChange::Set(score))?;
        }
        HistoryCommand::Adjust { path, delta } => {
            history.update(&normalize_path(&path)?, ScoreChange::Adjust(delta))?;
        }
        HistoryCommand::Remove { path } => {
            history.update(&normalize_path(&path)?, ScoreChange::Remove)?;
        }
        HistoryCommand::Prune => history.prune()?,
        HistoryCommand::Clear => history.clear()?,
    }
    Ok(())
}

fn print_entries(entries: impl IntoIterator<Item = HistoryEntry>) {
    let home = home();
    for entry in entries {
        let path = contract_tilde(&entry.path, home.as_deref());
        println!(
            "{}\t{}\t{}\t{}",
            entry.score,
            entry.frecency,
            format_age(entry.last_used),
            path.display()
        );
    }
}

fn format_age(age: std::time::Duration) -> String {
    let seconds = age.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 60 * 60 {
        format!("{}m", seconds / 60)
    } else if seconds < 24 * 60 * 60 {
        format!("{}h", seconds / (60 * 60))
    } else {
        format!("{}d", seconds / (24 * 60 * 60))
    }
}

fn find_projects(mut config: Config) -> Result<()> {
    init_logging(config.verbose).wrap_err("Failed to set up logging")?;

    // Ranking needs the complete result set before applying the output limit.
    let max_results = config.max_results.take();
    let output_format = config.output;
    let projects = ProjectFinder::new(config)
        .find_project_details()
        .wrap_err("Failed to find projects")?;
    let history = history_file_path()
        .map(History::open)
        .transpose()?
        .map_or_else(|| Ok(Vec::new()), |history| history.entries())?;
    let mut projects = rank_projects(projects, &history);
    if let Some(max) = max_results {
        projects.truncate(max.get());
    }

    let stdout = stdout();
    write_projects(
        &mut stdout.lock(),
        &projects,
        output_format,
        home().as_deref(),
    )?;

    Ok(())
}

fn add_project(path: &Path, resolver: &RootResolver) -> Result<()> {
    if !path.is_dir() {
        return Err(ProjectFinderError::PathNotFound(path.to_path_buf()).into());
    }
    let project = resolver.resolve_directory(&normalize_path(path)?)?;
    let history_path = history_file_path().ok_or(ProjectFinderError::HistoryLocationNotFound)?;
    History::open(history_path)?.record(&project)?;
    Ok(())
}

fn normalize_path(path: &Path) -> Result<std::path::PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|source| {
            ProjectFinderError::ResolvePath {
                path: path.to_path_buf(),
                source,
            }
            .into()
        });
    }

    std::path::absolute(path).map_err(|source| {
        ProjectFinderError::ResolvePath {
            path: path.to_path_buf(),
            source,
        }
        .into()
    })
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
