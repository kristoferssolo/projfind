//! Finds coding projects under a set of directories.
//!
//! A project is either a Git repository or a directory holding a recognised
//! marker file such as `Cargo.toml` or `package.json`. Markers are resolved to
//! the root they belong to, so a workspace reports once rather than once per
//! member.
//!
//! [`ProjectFinder`](finder::ProjectFinder) drives the search; [`Config`](config::Config)
//! decides where to look and what counts as a marker.

pub mod config;
pub mod dependencies;
pub mod errors;
pub mod finder;
pub mod scan;
