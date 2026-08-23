mod root;

use self::root::RootResolver;
use crate::{
    config::Config,
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
    scan::scan_directory,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    spawn,
    sync::{RwLock, Semaphore},
};
use tracing::{debug, info};

type ProjectSet = Arc<RwLock<HashSet<PathBuf>>>;

const MAX_CONCURRENT_SEARCHES: usize = 8;

fn is_covered_by(candidate: &Path, known: &Path) -> bool {
    if candidate == known {
        return true;
    }

    let is_direct_child = candidate.parent().is_some_and(|parent| parent == known);
    candidate.starts_with(known) && !is_direct_child
}

#[derive(Debug, Clone)]
pub struct ProjectFinder {
    config: Config,
    deps: Dependencies,
    discovered_projects: ProjectSet,
    root_resolver: RootResolver,
}

impl ProjectFinder {
    pub fn new(config: Config, deps: Dependencies) -> Self {
        let root_resolver = RootResolver::new(config.workspace_files.clone());

        Self {
            config,
            deps,
            discovered_projects: Arc::default(),
            root_resolver,
        }
    }

    pub async fn find_projects(&self) -> Result<Vec<PathBuf>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SEARCHES));
        let mut handles = Vec::with_capacity(self.config.paths.len());

        for path in &self.config.paths {
            if !path.is_dir() {
                return Err(ProjectFinderError::PathNotFound(path.clone()));
            }

            if self.config.verbose {
                info!("Searching in: {}", path.display());
            }

            let finder = self.clone();
            let path = path.clone();
            let semaphore = Arc::clone(&semaphore);

            handles.push((
                path.clone(),
                spawn(async move {
                    let _permit = semaphore.acquire().await.map_err(|source| {
                        ProjectFinderError::Scheduling {
                            path: path.clone(),
                            source,
                        }
                    })?;
                    finder.process_directory(&path).await
                }),
            ));
        }

        let mut errors = Vec::new();
        for (path, handle) in handles {
            let error = match handle.await {
                Ok(Ok(())) => continue,
                Ok(Err(error)) => error,
                Err(source) => ProjectFinderError::TaskFailed { path, source },
            };
            debug!("Search task failed: {error}");
            errors.push(error);
        }

        if errors.len() == self.config.paths.len()
            && let Some(error) = errors.into_iter().next()
        {
            return Err(error);
        }

        let mut projects = self
            .discovered_projects
            .read()
            .await
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        projects.sort();

        if let Some(max) = self.config.max_results {
            projects.truncate(max.get());
        }

        Ok(projects)
    }

    async fn process_directory(&self, dir: &Path) -> Result<()> {
        let scan = scan_directory(
            &self.deps,
            dir,
            &self.config.marker_files,
            self.config.depth,
        )
        .await?;

        self.discovered_projects
            .write()
            .await
            .extend(scan.git_repos);

        for (marker_name, paths) in scan.marker_files {
            for path in paths {
                if let Some(parent) = path.parent() {
                    self.process_marker(parent, &marker_name).await?;
                }
            }
        }

        Ok(())
    }

    async fn process_marker(&self, dir: &Path, marker_name: &str) -> Result<()> {
        let project_root = self.root_resolver.resolve(dir, marker_name).await?;
        let already_covered = self
            .discovered_projects
            .read()
            .await
            .iter()
            .any(|known| is_covered_by(&project_root, known));

        if !already_covered {
            self.discovered_projects.write().await.insert(project_root);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("/repo", "/repo", true, "the same directory is already known")]
    #[case("/repo/pkg", "/repo", false, "a direct child is a distinct project")]
    #[case(
        "/repo/pkg/inner",
        "/repo",
        true,
        "anything deeper belongs to the parent"
    )]
    #[case("/other", "/repo", false, "unrelated trees never cover each other")]
    fn coverage_rules(
        #[case] candidate: &str,
        #[case] known: &str,
        #[case] expected: bool,
        #[case] reason: &str,
    ) {
        assert_eq!(
            is_covered_by(Path::new(candidate), Path::new(known)),
            expected,
            "{reason}"
        );
    }
}
