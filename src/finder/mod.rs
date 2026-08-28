pub mod root;

use self::root::RootResolver;
use crate::{
    config::Config,
    errors::{ProjectFinderError, Result},
    scan::{DirectoryScan, scan_directories},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    hash::BuildHasher,
    path::{Path, PathBuf},
};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub path: PathBuf,
    pub markers: Vec<String>,
}

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
    root_resolver: RootResolver,
}

impl ProjectFinder {
    #[must_use]
    pub fn new(config: Config) -> Self {
        let root_resolver = RootResolver::from_config(&config);

        Self {
            config,
            root_resolver,
        }
    }

    /// Finds sorted project root paths.
    ///
    /// # Errors
    ///
    /// Returns an error if a configured path is not a directory, or if a
    /// marker cannot be resolved to a project root.
    pub fn find_projects(&self) -> Result<Vec<PathBuf>> {
        self.find_project_details()
            .map(|projects| projects.into_iter().map(|project| project.path).collect())
    }

    /// Finds sorted project roots and the markers that identified them.
    ///
    /// # Errors
    ///
    /// Returns an error if a configured path is not a directory, or if a
    /// marker cannot be resolved to a project root.
    pub fn find_project_details(&self) -> Result<Vec<Project>> {
        let scan = self.scan()?;

        let mut project_paths = scan.git_repos.iter().cloned().collect::<HashSet<_>>();
        let mut markers = project_paths
            .iter()
            .cloned()
            .map(|path| (path, BTreeSet::from([".git".to_owned()])))
            .collect::<HashMap<_, _>>();

        let mut candidates = self.resolve_marker_roots(&scan)?;
        candidates.sort_unstable();
        candidates.dedup();

        for (candidate, marker) in candidates {
            let project = candidate
                .ancestors()
                .skip(2)
                .find(|ancestor| project_paths.contains(*ancestor))
                .map_or_else(|| candidate.clone(), Path::to_path_buf);

            project_paths.insert(project.clone());
            markers.entry(project).or_default().insert(marker);
        }

        let mut projects = project_paths
            .into_iter()
            .map(|path| Project {
                markers: markers
                    .remove(&path)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                path,
            })
            .collect::<Vec<_>>();
        projects.sort_unstable_by(|left, right| left.path.cmp(&right.path));

        if let Some(max) = self.config.max_results {
            projects.truncate(max.get());
        }

        Ok(projects)
    }

    fn scan(&self) -> Result<DirectoryScan> {
        for path in &self.config.paths {
            if !path.is_dir() {
                return Err(ProjectFinderError::PathNotFound(path.clone()));
            }

            if self.config.verbose {
                info!("Searching in: {}", path.display());
            }
        }

        Ok(scan_directories(
            &self.config.paths,
            &self.config.marker_files,
            self.config.depth,
        ))
    }

    fn resolve_marker_roots(&self, scan: &DirectoryScan) -> Result<Vec<(PathBuf, String)>> {
        scan.marker_files
            .iter()
            .filter_map(|marker| {
                let dir = marker.parent()?;
                let name = marker.file_name()?.to_str()?;
                Some(
                    self.root_resolver
                        .resolve(dir, name)
                        .map(|root| (root, name.to_owned())),
                )
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
