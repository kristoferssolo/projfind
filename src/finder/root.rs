use crate::errors::{ProjectFinderError, Result};
use std::{
    collections::HashMap,
    future::Future,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs::{read_to_string, try_exists},
    sync::RwLock,
};

const CARGO_WORKSPACE: ContentTest = ContentTest::LineStartsWith("[workspace]");

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

type WorkspaceCache = Arc<RwLock<HashMap<PathBuf, bool>>>;
type RootCache = Arc<RwLock<HashMap<(PathBuf, MarkerType), PathBuf>>>;

#[derive(Debug, Clone)]
pub(super) struct RootResolver {
    workspace_files: Arc<[String]>,
    workspace_cache: WorkspaceCache,
    root_cache: RootCache,
}

impl RootResolver {
    pub(super) fn new(workspace_files: Vec<String>) -> Self {
        Self {
            workspace_files: workspace_files.into(),
            workspace_cache: Arc::default(),
            root_cache: Arc::default(),
        }
    }

    pub(super) async fn resolve(&self, dir: &Path, marker_name: &str) -> Result<PathBuf> {
        let marker_type = MarkerType::from(marker_name);
        let cache_key = (dir.to_path_buf(), marker_type.clone());

        if let Some(root) = self.root_cache.read().await.get(&cache_key) {
            return Ok(root.clone());
        }

        let root = match &marker_type {
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
            MarkerType::OtherConfig => ascend_to_root(dir, |_| async { Ok(false) }).await?,
        };

        self.root_cache
            .write()
            .await
            .insert(cache_key, root.clone());

        Ok(root)
    }

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
            for file in self.workspace_files.iter() {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MarkerType {
    PackageJson,
    CargoToml,
    DenoJson,
    BuildFile(String),
    OtherConfig,
}

impl From<&str> for MarkerType {
    fn from(file_name: &str) -> Self {
        match file_name {
            "package.json" => Self::PackageJson,
            "Cargo.toml" => Self::CargoToml,
            "deno.json" | "deno.jsonc" => Self::DenoJson,
            "Makefile" | "CMakeLists.txt" | "justfile" | "Justfile" => {
                Self::BuildFile(file_name.to_owned())
            }
            _ => Self::OtherConfig,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ContentTest {
    ContainsAny(&'static [&'static str]),
    LineStartsWith(&'static str),
    NonEmpty,
}

impl ContentTest {
    fn matches(self, contents: &str) -> bool {
        match self {
            Self::ContainsAny(needles) => needles.iter().any(|needle| contents.contains(needle)),
            Self::LineStartsWith(prefix) => contents
                .lines()
                .any(|line| line.trim_start().starts_with(prefix)),
            Self::NonEmpty => !contents.trim().is_empty(),
        }
    }
}

async fn file_matches(file: &Path, test: ContentTest) -> Result<bool> {
    match read_to_string(file).await {
        Ok(contents) => Ok(test.matches(&contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ProjectFinderError::read_file(file, error)),
    }
}

async fn path_exists(path: &Path) -> bool {
    try_exists(path).await.unwrap_or(false)
}

fn ancestors_above(dir: &Path) -> impl Iterator<Item = &Path> {
    dir.ancestors()
        .skip(1)
        .take_while(|ancestor| !ancestor.as_os_str().is_empty())
}

fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").is_dir()
}

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

    #[rstest]
    #[case("package.json", MarkerType::PackageJson)]
    #[case("Cargo.toml", MarkerType::CargoToml)]
    #[case("deno.json", MarkerType::DenoJson)]
    #[case("deno.jsonc", MarkerType::DenoJson)]
    #[case("justfile", MarkerType::BuildFile("justfile".into()))]
    #[case("CMakeLists.txt", MarkerType::BuildFile("CMakeLists.txt".into()))]
    #[case("go.mod", MarkerType::OtherConfig)]
    #[case("never-heard-of-it", MarkerType::OtherConfig)]
    fn classifies_markers(#[case] file_name: &str, #[case] expected: MarkerType) {
        assert_eq!(MarkerType::from(file_name), expected);
    }

    #[test]
    fn ancestors_skip_the_directory_itself() {
        let mut ancestors = ancestors_above(Path::new("/one/two/three"));

        assert_some_eq!(ancestors.next(), Path::new("/one/two"));
        assert_some_eq!(ancestors.next(), Path::new("/one"));
        assert_some_eq!(ancestors.next(), Path::new("/"));
        assert_none!(ancestors.next());
    }

    #[test]
    fn relative_ancestors_stop_before_the_empty_path() {
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
    async fn a_nearer_workspace_root_wins() {
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
    fn build_file_ascent_stops_at_the_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());
        write(leaf.join("Makefile"), "").expect("write leaf Makefile");
        write(temp.path().join("a/Makefile"), "").expect("write parent Makefile");

        assert_eq!(
            ascend_to_highest_build_file(&leaf, "Makefile"),
            temp.path().to_path_buf()
        );
    }

    #[test]
    fn build_file_ascent_without_a_repository_keeps_the_start() {
        let dir = Path::new("no-ancestors");
        assert_eq!(ascend_to_highest_build_file(dir, "Makefile"), dir);
    }

    #[test]
    fn contains_any_matches_a_single_needle() {
        let test = ContentTest::ContainsAny(&["\"workspaces\"", "\"workspace\""]);

        assert!(test.matches(r#"{"name": "x", "workspaces": ["a"]}"#));
        assert!(!test.matches(r#"{"name": "x"}"#));
    }

    #[test]
    fn line_starts_with_ignores_position_in_file() {
        let test = ContentTest::LineStartsWith("[workspace]");

        assert!(test.matches("# a comment\n[workspace]\nmembers = []"));
        assert!(test.matches("[workspace]"));
        assert!(!test.matches("[workspace.dependencies]\n"));
        assert!(!test.matches("[package]\nname = \"x\""));
    }

    #[test]
    fn non_empty_ignores_whitespace_only_files() {
        assert!(ContentTest::NonEmpty.matches("{}"));
        assert!(!ContentTest::NonEmpty.matches("  \n\t\n"));
    }
}
