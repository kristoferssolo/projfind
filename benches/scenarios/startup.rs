//! Fixed cost every subcommand pays before it does any work.
//!
//! The default configuration is embedded TOML that is parsed on each run, and
//! `mekle add` parses it too before it resolves a single directory.

use criterion::Criterion;
use mekle::config::Config;
use std::hint::black_box;

pub fn benchmark_startup(c: &mut Criterion) {
    c.bench_function("startup/default_config", |b| {
        b.iter(|| black_box(Config::defaults().expect("the built-in configuration parses")));
    });
}
