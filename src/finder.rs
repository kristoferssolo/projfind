use crate::{
    commands::{ContentTest, file_matches, find_files, find_git_repos},
    config::Config,
    dependencies::Dependencies,
    errors::{ProjectFinderError, Result},
    marker::{MARKER_FILES, MarkerType},
};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs::try_exists,
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

async fn path_exists(path: &Path) -> bool {
    try_exists(path).await.unwrap_or(false)
}

/// Ancestors nearest first, excluding the empty relative path.
fn ancestors_above(dir: &Path) -> impl Iterator<Item = &Path> {
    dir.ancestors()
        .skip(1)
        .take_while(|ancestor| !ancestor.as_os_str().is_empty())
}

/// Direct children remain separate projects. Deeper paths are covered.
fn is_covered_by(candidate: &Path, known: &Path) -> bool {
    if candidate == known {
        return true;
    }

    let is_direct_child = candidate.parent().is_some_and(|parent| parent == known);

    candidate.starts_with(known) && !is_direct_child
}

fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").is_dir()
}

/// Find the nearest workspace or Git root, falling back to `dir`.
async fn ascend_to_root<F, Fut>(dir: &Path, is_root: F) -> Result<PathBuf>
where
    F: Fn(PathBuf) -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    for parent in ancestors_above(dir) {
        if is_root(parent.to_path_buf()).await? || is_git_repo(parent) {
            return Ok(parent.to_path_buf());
        }
    }

    Ok(dir.to_path_buf())
}

/// Find the highest build-file ancestor within the enclosing repository.
fn ascend_to_highest_build_file(dir: &Path, build_file: &str) -> PathBuf {
    let mut highest = dir;

    for parent in ancestors_above(dir) {
        if parent.join(build_file).exists() {
            highest = parent;
        }

        if is_git_repo(parent) {
            return parent.to_path_buf();
        }
    }

    highest.to_path_buf()
}

#[derive(Debug, Clone)]
pub struct ProjectFinder {
    config: Config,
    deps: Dependencies,
    discovered_projects: ProjectSet,
    workspace_cache: WorkspaceCache,
    root_cache: RootCache,
}

impl ProjectFinder {
    pub fn new(config: Config, deps: Dependencies) -> Self {
        Self {
            config,
            deps,
            discovered_projects: Arc::new(RwLock::new(HashSet::new())),
            workspace_cache: Arc::new(RwLock::new(HashMap::new())),
            root_cache: Arc::new(RwLock::new(HashMap::new())),
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
                Ok(Err(e)) => e,
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
            .collect::<Vec<PathBuf>>();
        projects.sort();
        if let Some(max) = self.config.max_results {
            projects.truncate(max.get());
        }

        Ok(projects)
    }

    async fn process_directory(&self, dir: &Path) -> Result<()> {
        let (git_repos, marker_map) = tokio::try_join!(
            find_git_repos(&self.deps, dir, self.config.depth),
            find_files(&self.deps, dir, &MARKER_FILES, self.config.depth),
        )?;

        {
            let mut projects = self.discovered_projects.write().await;
            projects.extend(git_repos);
        }

        for (pattern, paths) in marker_map {
            for path in paths {
                if let Some(parent_dir) = path.parent() {
                    self.process_marker(parent_dir, &pattern).await?;
                }
            }
        }

        Ok(())
    }

    async fn process_marker(&self, dir: &Path, marker_name: &str) -> Result<()> {
        let marker_type = MarkerType::from(marker_name);
        let project_root = self.find_project_root(dir, &marker_type).await?;

        let already_covered = {
            let projects = self.discovered_projects.read().await;
            projects
                .iter()
                .any(|known| is_covered_by(&project_root, known))
        };

        if !already_covered {
            self.discovered_projects.write().await.insert(project_root);
        }

        Ok(())
    }

    /// Cache root resolution for sibling markers.
    async fn find_project_root(&self, dir: &Path, marker_type: &MarkerType) -> Result<PathBuf> {
        let cache_key = (dir.to_path_buf(), marker_type.clone());
        {
            let cache = self.root_cache.read().await;
            if let Some(root) = cache.get(&cache_key) {
                return Ok(root.clone());
            }
        }

        let root = match marker_type {
            MarkerType::PackageJson | MarkerType::DenoJson => {
                ascend_to_root(dir, |parent| async move {
                    self.is_workspace_root(&parent).await
                })
                .await?
            }
            MarkerType::CargoToml => {
                ascend_to_root(dir, |parent| async move {
                    file_matches(&parent.join("Cargo.toml"), CARGO_WORKSPACE).await
                })
                .await?
            }
            MarkerType::BuildFile(name) => ascend_to_highest_build_file(dir, name),
            MarkerType::OtherConfig(_) => ascend_to_root(dir, |_| async { Ok(false) }).await?,
        };

        self.root_cache
            .write()
            .await
            .insert(cache_key, root.clone());

        Ok(root)
    }

    /// Cache workspace checks during root ascent.
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

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok_eq, assert_some_eq};
    use rstest::rstest;
    use std::fs::{create_dir_all, write};
    use tempfile::TempDir;

    fn repo_with_nested_dirs(root: &Path) -> PathBuf {
        create_dir_all(root.join(".git")).expect("create .git");
        let leaf = root.join("a/b");
        create_dir_all(&leaf).expect("create nested dirs");
        leaf
    }

    fn holds(file: &'static str) -> impl Fn(PathBuf) -> std::future::Ready<Result<bool>> {
        move |dir: PathBuf| std::future::ready(Ok(dir.join(file).exists()))
    }

    #[test]
    fn ancestors_above_skips_the_directory_itself() {
        let dir = Path::new("/one/two/three");
        let mut ancestors = ancestors_above(dir);

        assert_some_eq!(ancestors.next(), Path::new("/one/two"));
        assert_some_eq!(ancestors.next(), Path::new("/one"));
        assert_some_eq!(ancestors.next(), Path::new("/"));
        assert_none!(ancestors.next());
    }

    #[test]
    fn ancestors_above_stops_before_the_empty_path() {
        assert_none!(ancestors_above(Path::new("relative")).next());
    }

    #[tokio::test]
    async fn ascent_stops_at_the_enclosing_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());

        assert_ok_eq!(
            ascend_to_root(&leaf, holds("never-exists")).await,
            temp.path().to_path_buf()
        );
    }

    #[tokio::test]
    async fn a_nearer_workspace_root_wins_over_the_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());
        write(temp.path().join("a/pnpm-workspace.yaml"), "").expect("write workspace file");

        assert_ok_eq!(
            ascend_to_root(&leaf, holds("pnpm-workspace.yaml")).await,
            temp.path().join("a")
        );
    }

    #[tokio::test]
    async fn ascent_falls_back_to_the_starting_directory() {
        let dir = Path::new("no-ancestors");

        assert_ok_eq!(
            ascend_to_root(dir, holds("never-exists")).await,
            dir.to_path_buf()
        );
    }

    #[test]
    fn build_file_ascent_takes_the_highest_within_the_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());
        write(leaf.join("Makefile"), "").expect("write leaf Makefile");
        write(temp.path().join("a/Makefile"), "").expect("write parent Makefile");

        assert_eq!(
            ascend_to_highest_build_file(&leaf, "Makefile"),
            temp.path().to_path_buf()
        );
    }

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

    #[test]
    fn build_file_ascent_without_a_repository_keeps_the_starting_directory() {
        let dir = Path::new("no-ancestors");

        assert_eq!(ascend_to_highest_build_file(dir, "Makefile"), dir);
    }
}
