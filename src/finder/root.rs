use super::is_covered;
use crate::{
    config::Config,
    error::{Error, Result},
    git::{GIT_DIR, marks_repository},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::read_to_string,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::RwLock,
};

const CARGO_WORKSPACE: ContentTest = ContentTest::LineStartsWith("[workspace]");

const WORKSPACE_RULES: [(&str, ContentTest); 8] = [
    (
        "package.json",
        ContentTest::ContainsAny(&[r#""workspaces""#, r#""workspace""#]),
    ),
    (
        "deno.json",
        ContentTest::ContainsAny(&[r#""workspaces""#, r#""imports""#]),
    ),
    (
        "deno.jsonc",
        ContentTest::ContainsAny(&[r#""workspaces""#, r#""imports""#]),
    ),
    ("bunfig.toml", ContentTest::ContainsAny(&["workspaces"])),
    ("Cargo.toml", CARGO_WORKSPACE),
    ("rush.json", ContentTest::NonEmpty),
    ("nx.json", ContentTest::NonEmpty),
    ("turbo.json", ContentTest::NonEmpty),
];

const POISONED: &str = "resolver cache lock poisoned";

#[derive(Debug)]
pub struct RootResolver {
    marker_files: Box<[String]>,
    workspace_files: Box<[String]>,
    workspace_cache: RwLock<HashMap<PathBuf, bool>>,
    root_cache: RwLock<HashMap<(PathBuf, MarkerType), PathBuf>>,
}

impl RootResolver {
    #[must_use]
    pub fn new(workspace_files: Vec<String>) -> Self {
        Self {
            marker_files: Box::default(),
            workspace_files: workspace_files.into(),
            workspace_cache: RwLock::default(),
            root_cache: RwLock::default(),
        }
    }

    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        Self {
            marker_files: config.marker_files.clone().into(),
            workspace_files: config.workspace_files.clone().into(),
            workspace_cache: RwLock::default(),
            root_cache: RwLock::default(),
        }
    }

    /// Resolves an arbitrary directory to the most specific project root that
    /// discovery would report for one of its ancestors.
    ///
    /// # Errors
    ///
    /// Returns an error if a marker or workspace manifest cannot be inspected.
    ///
    /// # Panics
    ///
    /// Panics if a cache lock is poisoned.
    pub fn resolve_directory(&self, dir: &Path) -> Result<PathBuf> {
        let mut projects = HashSet::new();
        let mut candidates = BTreeSet::new();

        for ancestor in dir.ancestors() {
            if is_git_repo(ancestor)? {
                projects.insert(ancestor.to_path_buf());
            }

            for marker_name in &self.marker_files {
                let marker = ancestor.join(marker_name);
                if marker
                    .try_exists()
                    .map_err(|source| Error::read_file(&marker, source))?
                {
                    candidates.insert(self.resolve(ancestor, marker_name)?);
                }
            }
        }

        for candidate in candidates {
            if !is_covered(&candidate, &projects) {
                projects.insert(candidate);
            }
        }

        Ok(projects
            .into_iter()
            .filter(|candidate| dir.starts_with(candidate))
            .max_by_key(|candidate| candidate.components().count())
            .unwrap_or_else(|| dir.to_path_buf()))
    }

    /// Resolves a marker's project root.
    ///
    /// # Errors
    ///
    /// Returns an error if a workspace manifest cannot be read.
    ///
    /// # Panics
    ///
    /// Panics if a cache lock is poisoned.
    pub fn resolve(&self, dir: &Path, marker_name: &str) -> Result<PathBuf> {
        let cache_key = (dir.to_path_buf(), MarkerType::from(marker_name));

        if let Some(root) = self.root_cache.read().expect(POISONED).get(&cache_key) {
            return Ok(root.clone());
        }

        let root = match &cache_key.1 {
            MarkerType::PackageJson | MarkerType::DenoJson => {
                ascend_to_root(dir, |parent| self.is_workspace_root(parent))?
            }
            MarkerType::CargoToml => ascend_to_root(dir, |parent| {
                file_matches(&parent.join("Cargo.toml"), CARGO_WORKSPACE)
            })?,
            MarkerType::BuildFile(name) => ascend_to_highest_build_file(dir, name)?,
            MarkerType::OtherConfig => ascend_to_root(dir, |_| Ok(false))?,
        };

        self.root_cache
            .write()
            .expect(POISONED)
            .insert(cache_key, root.clone());

        Ok(root)
    }

    fn is_workspace_root(&self, dir: &Path) -> Result<bool> {
        if let Some(&cached) = self.workspace_cache.read().expect(POISONED).get(dir) {
            return Ok(cached);
        }

        let mut is_root = false;
        for (file, test) in WORKSPACE_RULES {
            if file_matches(&dir.join(file), test)? {
                is_root = true;
                break;
            }
        }

        if !is_root {
            is_root = self
                .workspace_files
                .iter()
                .any(|file| dir.join(file).exists());
        }

        self.workspace_cache
            .write()
            .expect(POISONED)
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

fn file_matches(file: &Path, test: ContentTest) -> Result<bool> {
    match read_to_string(file) {
        Ok(contents) => Ok(test.matches(&contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::read_file(file, error)),
    }
}

fn ancestors_above(dir: &Path) -> impl Iterator<Item = &Path> {
    dir.ancestors()
        .skip(1)
        .take_while(|ancestor| !ancestor.as_os_str().is_empty())
}

/// Reports whether `dir` is the root of a repository, worktree, or submodule.
fn is_git_repo(dir: &Path) -> Result<bool> {
    marks_repository(&dir.join(GIT_DIR))
}

fn ascend_to_root<F>(dir: &Path, is_root: F) -> Result<PathBuf>
where
    F: Fn(&Path) -> Result<bool>,
{
    for parent in ancestors_above(dir) {
        if is_git_repo(parent)? || is_root(parent)? {
            return Ok(parent.to_path_buf());
        }
    }

    Ok(dir.to_path_buf())
}

fn ascend_to_highest_build_file(dir: &Path, build_file: &str) -> Result<PathBuf> {
    let mut highest = dir;

    for parent in ancestors_above(dir) {
        if parent.join(build_file).exists() {
            highest = parent;
        }

        if is_git_repo(parent)? {
            return Ok(parent.to_path_buf());
        }
    }

    Ok(highest.to_path_buf())
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

    fn holds(file: &'static str) -> impl Fn(&Path) -> Result<bool> {
        move |dir: &Path| Ok(dir.join(file).exists())
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

    #[test]
    fn ascent_stops_at_the_enclosing_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());

        assert_ok_eq!(
            ascend_to_root(&leaf, holds("never-exists")),
            temp.path().to_path_buf()
        );
    }

    #[test]
    fn a_nearer_workspace_root_wins() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());
        write(temp.path().join("a/pnpm-workspace.yaml"), "").expect("write workspace file");

        assert_ok_eq!(
            ascend_to_root(&leaf, holds("pnpm-workspace.yaml")),
            temp.path().join("a")
        );
    }

    #[test]
    fn directory_resolution_uses_discovery_coverage_rules() {
        let temp = TempDir::new().expect("create temp dir");
        let service = temp.path().join("service");
        let tool = service.join("tool");
        let plugin = tool.join("plugin");
        let nested = plugin.join("src");
        for project in [&service, &tool, &plugin] {
            create_dir_all(project).expect("create project dir");
            write(project.join("go.mod"), "module example\n").expect("write marker");
        }
        create_dir_all(&nested).expect("create nested dir");
        let mut config = Config::defaults().expect("load default config");
        config.marker_files = vec!["go.mod".to_owned()];
        let resolver = RootResolver::from_config(&config);

        assert_ok_eq!(resolver.resolve_directory(&nested), tool);
    }

    #[test]
    fn a_worktree_outranks_the_repository_and_workspace_around_it() {
        let temp = TempDir::new().expect("create temp dir");
        create_dir_all(temp.path().join(GIT_DIR)).expect("create .git");
        write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"feature/crates/*\"]\n",
        )
        .expect("write workspace manifest");

        let worktree = temp.path().join("feature");
        let admin = temp.path().join(".git/worktrees/feature");
        create_dir_all(&admin).expect("create worktree admin dir");
        let member = worktree.join("crates/one");
        create_dir_all(&member).expect("create member dir");
        write(
            worktree.join(GIT_DIR),
            format!("gitdir: {}\n", admin.display()),
        )
        .expect("write worktree .git file");
        write(member.join("Cargo.toml"), "[package]\nname = \"one\"\n")
            .expect("write member manifest");

        let resolver = RootResolver::new(Vec::new());

        assert_ok_eq!(resolver.resolve(&member, "Cargo.toml"), worktree);
    }

    #[test]
    fn ascent_falls_back_to_the_starting_directory() {
        let dir = Path::new("no-ancestors");

        assert_ok_eq!(
            ascend_to_root(dir, holds("never-exists")),
            dir.to_path_buf()
        );
    }

    #[test]
    fn build_file_ascent_stops_at_the_repository() {
        let temp = TempDir::new().expect("create temp dir");
        let leaf = repo_with_nested_dirs(temp.path());
        write(leaf.join("Makefile"), "").expect("write leaf Makefile");
        write(temp.path().join("a/Makefile"), "").expect("write parent Makefile");

        assert_ok_eq!(
            ascend_to_highest_build_file(&leaf, "Makefile"),
            temp.path().to_path_buf()
        );
    }

    #[test]
    fn build_file_ascent_without_a_repository_keeps_the_start() {
        let dir = Path::new("no-ancestors");
        assert_ok_eq!(
            ascend_to_highest_build_file(dir, "Makefile"),
            dir.to_path_buf()
        );
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
