use crate::{
    commands::{ContentTest, file_matches, find_files, find_git_repos},
    config::Config,
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
    marker::{MARKER_FILES, MarkerType},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs::metadata,
    spawn,
    sync::{RwLock, Semaphore},
};
use tracing::{debug, info};

type ProjectSet = Arc<RwLock<HashSet<PathBuf>>>;
type WorkspaceCache = Arc<RwLock<HashMap<PathBuf, bool>>>;
type RootCache = Arc<RwLock<HashMap<(PathBuf, MarkerType), PathBuf>>>;

/// Upper bound on directories searched at once, so a long path list cannot
/// spawn an unbounded number of `fd` processes.
const MAX_CONCURRENT_SEARCHES: usize = 8;

/// A `Cargo.toml` declaring a workspace.
const CARGO_WORKSPACE: ContentTest = ContentTest::LineStartsWith("[workspace]");

/// Files that mark a workspace root only when their contents say so.
const WORKSPACE_RULES: [(&str, ContentTest); 8] = [
    (
        "package.json",
        ContentTest::ContainsAny(&["\"workspaces\"", "\"workspace\""]),
    ),
    (
        "deno.json",
        ContentTest::ContainsAny(&["\"workspaces\"", "\"imports\""]),
    ),
    (
        "deno.jsonc",
        ContentTest::ContainsAny(&["\"workspaces\"", "\"imports\""]),
    ),
    ("bunfig.toml", ContentTest::ContainsAny(&["workspaces"])),
    ("Cargo.toml", CARGO_WORKSPACE),
    ("rush.json", ContentTest::NonEmpty),
    ("nx.json", ContentTest::NonEmpty),
    ("turbo.json", ContentTest::NonEmpty),
];

/// Files that mark a workspace root just by existing.
const WORKSPACE_FILES: [&str; 5] = [
    "pnpm-workspace.yaml",
    "lerna.json",
    "yarn.lock",      // Common in yarn workspaces
    ".yarnrc.yml",    // Yarn 2+ workspaces
    "workspace.json", // Generic workspace file
];

/// Check whether a given path exists.
async fn path_exists(path: &Path) -> bool {
    metadata(path).await.is_ok()
}

/// Struct responsible for scanning directories and detecting projects.
#[derive(Debug, Clone)]
pub struct ProjectFinder {
    config: Config,
    deps: Dependencies,
    discovered_projects: ProjectSet,
    workspace_cache: WorkspaceCache,
    root_cache: RootCache,
}

impl ProjectFinder {
    /// Create a new `ProjectFinder` instance.
    pub fn new(config: Config, deps: Dependencies) -> Self {
        Self {
            config,
            deps,
            discovered_projects: Arc::new(RwLock::new(HashSet::new())),
            workspace_cache: Arc::new(RwLock::new(HashMap::new())),
            root_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Find projects in the configured paths.
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

        // Await every task, keeping the errors so a total failure can be reported.
        let mut errors = Vec::new();
        for (path, handle) in handles {
            let error = match handle.await {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => e,
                Err(source) => ProjectFinderError::TaskFailed { path, source },
            };
            debug!("Search task failed: {error}");
            errors.push(error);
        }

        // A partial failure still reports what the surviving paths found; only a
        // total failure is fatal.
        if errors.len() == self.config.paths.len()
            && let Some(error) = errors.into_iter().next()
        {
            return Err(error);
        }

        // Gather discovered projects, sort and apply max_results limit, if set.
        let mut projects = self
            .discovered_projects
            .read()
            .await
            .iter()
            .cloned()
            .collect::<Vec<PathBuf>>();
        projects.sort();
        if self.config.max_results > 0 && projects.len() > self.config.max_results {
            projects.truncate(self.config.max_results);
        }

        Ok(projects)
    }

    /// Process a single directory by scanning for git repositories and marker files.
    async fn process_directory(&self, dir: &Path) -> Result<()> {
        // Look for git repositories first.
        let git_repos = find_git_repos(&self.deps, dir, self.config.depth).await?;

        {
            let mut projects = self.discovered_projects.write().await;
            projects.extend(git_repos);
        }

        // Look for marker files.
        let marker_map = find_files(&self.deps, dir, &MARKER_FILES, self.config.depth).await?;
        for (pattern, paths) in marker_map {
            for path in paths {
                if let Some(parent_dir) = path.parent() {
                    self.process_marker(parent_dir, &pattern).await?;
                }
            }
        }

        Ok(())
    }

    /// Process a marker file found in a directory.
    async fn process_marker(&self, dir: &Path, marker_name: &str) -> Result<()> {
        let marker_type = MarkerType::from(marker_name);

        // Find project root
        let project_root = self.find_project_root(dir, &marker_type).await?;

        // Improved nested project detection
        // Only ignore if it's a subproject of the same type (prevents ignoring
        // valid nested projects of different types)
        let mut should_add = true;
        {
            let projects = self.discovered_projects.read().await;
            for known_project in projects.iter() {
                // Check if this is a direct parent (not just any ancestor)
                let is_direct_parent = project_root
                    .parent()
                    .is_some_and(|parent| parent == known_project);

                // Only exclude if it's a subdirectory and has the same marker type
                // or if it's exactly the same directory
                if project_root == *known_project
                    || project_root.starts_with(known_project) && !is_direct_parent
                {
                    should_add = false;
                    break;
                }
            }
        }

        if should_add {
            self.discovered_projects.write().await.insert(project_root);
        }

        Ok(())
    }

    async fn find_project_root(&self, dir: &Path, marker_type: &MarkerType) -> Result<PathBuf> {
        let cache_key = (dir.to_path_buf(), marker_type.clone());
        {
            let cache = self.root_cache.read().await;
            if let Some(root) = cache.get(&cache_key) {
                return Ok(root.clone());
            }
        }

        let mut result = dir.to_path_buf();

        match marker_type {
            MarkerType::PackageJson | MarkerType::DenoJson => {
                // Check for workspace roots
                let mut current = dir.to_path_buf();
                while let Some(parent) = current.parent() {
                    if parent.as_os_str().is_empty() {
                        break;
                    }

                    if self.is_workspace_root(parent).await? {
                        result = parent.to_path_buf();
                        break;
                    }

                    if parent.join(".git").is_dir() {
                        result = parent.to_path_buf();
                        break;
                    }

                    current = parent.to_path_buf();
                }
            }

            MarkerType::CargoToml => {
                // Check for Cargo workspace
                let mut current = dir.to_path_buf();
                while let Some(parent) = current.parent() {
                    if parent.as_os_str().is_empty() {
                        break;
                    }

                    let cargo_toml = parent.join("Cargo.toml");
                    if file_matches(&cargo_toml, CARGO_WORKSPACE).await? {
                        result = parent.to_path_buf();
                        break;
                    }

                    if parent.join(".git").is_dir() {
                        result = parent.to_path_buf();
                        break;
                    }

                    current = parent.to_path_buf();
                }
            }

            MarkerType::BuildFile(name) => {
                // For build system files, find the highest one that's still in the same git repo
                let mut highest_dir = dir.to_path_buf();
                let mut current = dir.to_path_buf();

                while let Some(parent) = current.parent() {
                    if parent.as_os_str().is_empty() {
                        break;
                    }

                    if parent.join(name).exists() {
                        highest_dir = parent.to_path_buf();
                    }

                    if parent.join(".git").is_dir() {
                        result = parent.to_path_buf();
                        break;
                    }

                    current = parent.to_path_buf();
                }

                if result == dir.to_path_buf() {
                    result = highest_dir;
                }
            }

            MarkerType::OtherConfig(_) => {
                // For other file types, just look for git repos
                let mut current = dir.to_path_buf();
                while let Some(parent) = current.parent() {
                    if parent.as_os_str().is_empty() {
                        break;
                    }

                    if parent.join(".git").is_dir() {
                        result = parent.to_path_buf();
                        break;
                    }

                    current = parent.to_path_buf();
                }
            }
        }

        // Cache the result
        self.root_cache
            .write()
            .await
            .insert(cache_key, result.clone());

        Ok(result)
    }

    /// Check whether `dir` is the root of a multi-package workspace.
    ///
    /// Results are cached because the ascent in [`Self::find_project_root`]
    /// revisits the same ancestors for every marker found beneath them.
    async fn is_workspace_root(&self, dir: &Path) -> Result<bool> {
        if let Some(&cached) = self.workspace_cache.read().await.get(dir) {
            return Ok(cached);
        }

        let mut is_root = false;
        for (file, test) in WORKSPACE_RULES {
            if file_matches(&dir.join(file), test).await? {
                is_root = true;
                break;
            }
        }

        if !is_root {
            for file in WORKSPACE_FILES {
                if path_exists(&dir.join(file)).await {
                    is_root = true;
                    break;
                }
            }
        }

        self.workspace_cache
            .write()
            .await
            .insert(dir.to_path_buf(), is_root);

        Ok(is_root)
    }
}
