//! Gitignore-style exclusions, applied per search directory.

use crate::error::{Error, Result};
use ignore::{Match, gitignore::Gitignore, gitignore::GitignoreBuilder};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct Exclusions(Vec<Exclusion>);

#[derive(Debug)]
struct Exclusion {
    root: PathBuf,
    matcher: Gitignore,
}

impl Exclusions {
    pub(super) fn new(roots: &[PathBuf], patterns: &[String]) -> Result<Self> {
        roots
            .iter()
            .map(|root| Exclusion::new(root, patterns))
            .collect::<Result<Vec<_>>>()
            .map(Self)
    }

    pub(super) fn matches(&self, path: &Path, is_dir: bool) -> bool {
        // `ignore` strips roots as bytes on Unix, so check components first.
        self.0.iter().any(|exclusion| {
            path.strip_prefix(&exclusion.root).is_ok()
                && matches!(exclusion.matcher.matched(path, is_dir), Match::Ignore(_))
        })
    }
}

impl Exclusion {
    fn new(root: &Path, patterns: &[String]) -> Result<Self> {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            builder
                .add_line(None, pattern)
                .map_err(|source| Error::InvalidExcludePattern {
                    pattern: pattern.clone(),
                    source,
                })?;
        }

        let matcher = builder
            .build()
            .map_err(|source| Error::InvalidExcludeSet { source })?;
        Ok(Self {
            root: root.to_path_buf(),
            matcher,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    fn exclusions(roots: &[&str], patterns: &[&str]) -> Result<Exclusions> {
        Exclusions::new(
            &roots.iter().map(PathBuf::from).collect::<Vec<_>>(),
            &patterns
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_matcher_cannot_reach_a_sibling_with_a_prefix_name() {
        let exclusions = assert_ok!(exclusions(&["/repos/bar", "/repos/barley"], &["/ley/**"]));

        assert!(!exclusions.matches(Path::new("/repos/barley/repo"), true));
    }

    #[test]
    fn invalid_patterns_name_the_rejected_pattern() {
        let error = assert_err!(exclusions(&["/repos"], &["[z-a]"]));

        assert!(
            error.to_string().contains("[z-a]"),
            "error does not name the pattern: {error}"
        );
    }
}
