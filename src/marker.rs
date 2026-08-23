/// File names that identify projects.
pub const MARKER_FILES: [&str; 13] = [
    "package.json",
    "pnpm-workspace.yaml",
    "lerna.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "CMakeLists.txt",
    "Makefile",
    "justfile",
    "Justfile",
    "deno.json",
    "deno.jsonc",
    "bunfig.toml",
];

/// How a marker determines its project root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MarkerType {
    PackageJson,
    CargoToml,
    DenoJson,
    BuildFile(String),
    OtherConfig(String),
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
            other => Self::OtherConfig(other.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("package.json", MarkerType::PackageJson)]
    #[case("Cargo.toml", MarkerType::CargoToml)]
    #[case("deno.json", MarkerType::DenoJson)]
    #[case("deno.jsonc", MarkerType::DenoJson)]
    #[case("justfile", MarkerType::BuildFile("justfile".into()))]
    #[case("CMakeLists.txt", MarkerType::BuildFile("CMakeLists.txt".into()))]
    #[case("go.mod", MarkerType::OtherConfig("go.mod".into()))]
    fn classifies_known_markers(#[case] file_name: &str, #[case] expected: MarkerType) {
        assert_eq!(MarkerType::from(file_name), expected);
    }

    #[test]
    fn unknown_names_fall_back_to_other_config() {
        assert_eq!(
            MarkerType::from("never-heard-of-it"),
            MarkerType::OtherConfig("never-heard-of-it".into())
        );
    }
}
