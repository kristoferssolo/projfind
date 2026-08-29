//! Fast project discovery.
//!
//! A run walks the configured search directories once ([`scan`]), decides which
//! directory each marker really belongs to ([`finder`]), ranks the results
//! against recorded visits ([`history`]), and prints them ([`output`]).
//!
//! Every filesystem read and write goes through [`fs`], so the rest of the
//! crate asks questions about paths without handling `io::Error` itself.

pub mod completions;
pub mod config;
pub mod error;
pub mod finder;
pub mod fs;
pub mod git;
pub mod history;
pub mod output;
pub mod paths;
pub mod scan;
pub mod shell;
