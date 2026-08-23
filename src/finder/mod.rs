pub mod root;

use self::root::RootResolver;
use crate::{
    config::Config,
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
    scan::{DirectoryScan, scan_directory},
};
use std::{
    collections::HashSet,
    hash::BuildHasher,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{spawn, sync::Semaphore};
use tracing::{debug, info};

const MAX_CONCURRENT_SEARCHES: usize = 8;

#[must_use]
pub fn is_covered<S: BuildHasher>(candidate: &Path, known: &HashSet<PathBuf, S>) -> bool {
    known.contains(candidate)
        || candidate
            .ancestors()
            .skip(2)
            .any(|ancestor| known.contains(ancestor))
}

#[derive(Debug)]
pub struct ProjectFinder {
    config: Config,
    deps: Dependencies,
    root_resolver: RootResolver,
}

impl ProjectFinder {
    #[must_use]
    pub fn new(config: Config, deps: Dependencies) -> Self {
        let root_resolver = RootResolver::new(config.workspace_files.clone());

        Self {
            config,
            deps,
            root_resolver,
        }
    }

    /// Finds sorted project roots.
    ///
    /// # Errors
    ///
    /// Returns an error if no configured directory can be searched.
    pub async fn find_projects(&self) -> Result<Vec<PathBuf>> {
        let scans = self.scan_all().await?;

        let mut projects = scans
            .iter()
            .flat_map(|scan| scan.git_repos.iter().cloned())
            .collect::<HashSet<_>>();

        let mut candidates = self.resolve_marker_roots(&scans)?;
        candidates.sort_unstable();
        candidates.dedup();

        for candidate in candidates {
            if !is_covered(&candidate, &projects) {
                projects.insert(candidate);
            }
        }

        let mut projects = projects.into_iter().collect::<Vec<_>>();
        projects.sort_unstable();

        if let Some(max) = self.config.max_results {
            projects.truncate(max.get());
        }

        Ok(projects)
    }

    async fn scan_all(&self) -> Result<Vec<DirectoryScan>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SEARCHES));
        let mut handles = Vec::with_capacity(self.config.paths.len());

        for path in &self.config.paths {
            if !path.is_dir() {
                return Err(ProjectFinderError::PathNotFound(path.clone()));
            }

            if self.config.verbose {
                info!("Searching in: {}", path.display());
            }

            let deps = self.deps.clone();
            let marker_files = self.config.marker_files.clone();
            let depth = self.config.depth;
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
                    scan_directory(&deps, &path, &marker_files, depth).await
                }),
            ));
        }

        let mut scans = Vec::with_capacity(handles.len());
        let mut errors = Vec::new();
        for (path, handle) in handles {
            let error = match handle.await {
                Ok(Ok(scan)) => {
                    scans.push(scan);
                    continue;
                }
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

        Ok(scans)
    }

    fn resolve_marker_roots(&self, scans: &[DirectoryScan]) -> Result<Vec<PathBuf>> {
        scans
            .iter()
            .flat_map(|scan| scan.marker_files.iter())
            .filter_map(|marker| {
                let dir = marker.parent()?;
                let name = marker.file_name()?.to_str()?;
                Some(self.root_resolver.resolve(dir, name))
            })
            .collect()
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
        let known = HashSet::from([PathBuf::from(known)]);

        assert_eq!(
            is_covered(Path::new(candidate), &known),
            expected,
            "{reason}"
        );
    }
}
