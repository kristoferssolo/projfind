//! Turning a directory walk into the list of projects it found.

pub mod root;

use self::root::RootResolver;
use crate::{
    config::Config,
    error::{Error, Result},
    git::GIT_DIR,
    scan::{DirectoryScan, scan_directories},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    hash::BuildHasher,
    path::{Path, PathBuf},
};
use tracing::info;

/// A directory worth reporting, and the markers that identified it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub path: PathBuf,
    pub markers: Vec<String>,
}

/// The ancestors that could absorb `candidate` into a project of their own.
///
/// A project only covers what lies two or more levels below it, so a direct
/// child of a project is still a project in its own right. That is what keeps
/// the members of a monorepo visible while their `src` directories stay hidden.
fn covering_ancestors(candidate: &Path) -> impl Iterator<Item = &Path> {
    candidate.ancestors().skip(2)
}

/// Reports whether one of the `known` projects already accounts for
/// `candidate`.
#[must_use]
pub fn is_covered<S: BuildHasher>(candidate: &Path, known: &HashSet<PathBuf, S>) -> bool {
    known.contains(candidate)
        || covering_ancestors(candidate).any(|ancestor| known.contains(ancestor))
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
    /// Returns an error if a configured path is not a directory, if an
    /// exclusion pattern is invalid, or if a marker cannot be resolved to a
    /// project root.
    pub fn find_projects(&self) -> Result<Vec<PathBuf>> {
        self.find_project_details()
            .map(|projects| projects.into_iter().map(|project| project.path).collect())
    }

    /// Finds sorted project roots and the markers that identified them.
    ///
    /// # Errors
    ///
    /// Returns an error if a configured path is not a directory, if an
    /// exclusion pattern is invalid, or if a marker cannot be resolved to a
    /// project root.
    pub fn find_project_details(&self) -> Result<Vec<Project>> {
        let scan = self.scan()?;

        // Repositories are projects outright; every other marker has to be
        // resolved to a root first, and may land on a repository already here.
        let mut projects = scan
            .git_repos
            .iter()
            .map(|repo| (repo.clone(), BTreeSet::from([GIT_DIR.to_owned()])))
            .collect::<HashMap<_, BTreeSet<String>>>();

        let mut candidates = self.resolve_marker_roots(&scan)?;
        candidates.sort_unstable();
        candidates.dedup();

        for (candidate, marker) in candidates {
            let root = covering_ancestors(&candidate)
                .find(|ancestor| projects.contains_key(*ancestor))
                .map(Path::to_path_buf);

            projects
                .entry(root.unwrap_or(candidate))
                .or_default()
                .insert(marker);
        }

        let mut projects = projects
            .into_iter()
            .map(|(path, markers)| Project {
                path,
                markers: markers.into_iter().collect(),
            })
            .collect::<Vec<_>>();
        projects.sort_unstable_by(|left, right| left.path.cmp(&right.path));

        Ok(projects)
    }

    fn scan(&self) -> Result<DirectoryScan> {
        for path in &self.config.paths {
            if !path.is_dir() {
                return Err(Error::PathNotFound(path.clone()));
            }

            info!("Searching in: {}", path.display());
        }

        scan_directories(
            &self.config.paths,
            &self.config.marker_files,
            self.config.depth,
            &self.config.exclude,
        )
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
