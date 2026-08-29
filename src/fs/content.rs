//! Tests applied to the contents of a file.
//!
//! These say *how* to look at a file, not *what* a match means. The rules that
//! give a match meaning, such as which manifests declare a workspace, belong to
//! the module that owns that decision.

use crate::{error::Result, fs};
use std::path::Path;

/// A question asked of a file's contents.
#[derive(Debug, Clone, Copy)]
pub enum ContentTest {
    /// Holds any one of these substrings.
    ContainsAny(&'static [&'static str]),
    /// Has a line that begins with this prefix, ignoring leading whitespace.
    LineStartsWith(&'static str),
    /// Holds anything other than whitespace.
    NonEmpty,
}

impl ContentTest {
    /// Reports whether `contents` answers this test.
    #[must_use]
    pub fn matches(self, contents: &str) -> bool {
        match self {
            Self::ContainsAny(needles) => needles.iter().any(|needle| contents.contains(needle)),
            Self::LineStartsWith(prefix) => contents
                .lines()
                .any(|line| line.trim_start().starts_with(prefix)),
            Self::NonEmpty => !contents.trim().is_empty(),
        }
    }

    /// Applies this test to the contents of `file`. A missing file matches
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if `file` exists but cannot be read.
    pub fn matches_file(self, file: &Path) -> Result<bool> {
        Ok(fs::read(file)?.is_some_and(|contents| self.matches(&contents)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok_eq;
    use std::fs::write;
    use tempfile::TempDir;

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

    #[test]
    fn a_missing_file_matches_nothing() {
        let temp = TempDir::new().expect("create temp dir");

        assert_ok_eq!(
            ContentTest::NonEmpty.matches_file(&temp.path().join("absent")),
            false
        );
    }

    #[test]
    fn an_existing_file_is_judged_by_its_contents() {
        let temp = TempDir::new().expect("create temp dir");
        let manifest = temp.path().join("Cargo.toml");
        write(&manifest, "[workspace]\nmembers = []\n").expect("write manifest");

        assert_ok_eq!(
            ContentTest::LineStartsWith("[workspace]").matches_file(&manifest),
            true
        );
        assert_ok_eq!(
            ContentTest::LineStartsWith("[package]").matches_file(&manifest),
            false
        );
    }
}
