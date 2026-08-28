//! Walking a marker, or an arbitrary directory, up to the project that owns it.
//!
//! Cold measurements rebuild the resolver every iteration because a process
//! only ever runs one discovery pass; the warm variant is there to show what
//! the cache buys within that single pass.

use crate::common::tree;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use mekle::{config::Config, finder::root::RootResolver};
use std::{hint::black_box, path::PathBuf};
use tempfile::TempDir;

const MEMBER_COUNTS: [usize; 3] = [16, 128, 1024];
const NESTING_LEVELS: [usize; 3] = [4, 16, 64];

/// Repositories in the tree used to time coverage folding.
const VENDORED_REPOS: usize = 32;

pub fn benchmark_root_resolution(c: &mut Criterion) {
    let workspace_files = defaults().workspace_files;

    let mut group = c.benchmark_group("root_resolution");

    for members in MEMBER_COUNTS {
        let temp = TempDir::new().expect("create a temporary directory");
        let markers = tree::monorepo(temp.path(), members).expect("build the monorepo");

        group.throughput(Throughput::Elements(members as u64));

        group.bench_function(BenchmarkId::new("cold_cache", members), |b| {
            b.iter_batched(
                || RootResolver::new(workspace_files.clone()),
                |resolver| black_box(resolve_all(&resolver, &markers)),
                BatchSize::PerIteration,
            );
        });

        let resolver = RootResolver::new(workspace_files.clone());
        resolve_all(&resolver, &markers);
        group.bench_function(BenchmarkId::new("warm_cache", members), |b| {
            b.iter(|| black_box(resolve_all(&resolver, &markers)));
        });
    }

    group.finish();
}

/// One marker resolved from `levels` directories down, so the reported time is
/// the latency of a single ascent rather than a rate.
pub fn benchmark_deep_ascent(c: &mut Criterion) {
    let workspace_files = defaults().workspace_files;

    let mut group = c.benchmark_group("deep_ascent");

    for levels in NESTING_LEVELS {
        let temp = TempDir::new().expect("create a temporary directory");
        let markers = tree::deep_nesting(temp.path(), levels).expect("build the nested tree");

        group.bench_function(BenchmarkId::from_parameter(levels), |b| {
            b.iter_batched(
                || RootResolver::new(workspace_files.clone()),
                |resolver| black_box(resolve_all(&resolver, &markers)),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// `mekle add` resolves the current directory on every jump the shell records,
/// probing each configured marker at every ancestor. This is that latency.
pub fn benchmark_directory_resolution(c: &mut Criterion) {
    let config = defaults();

    let mut group = c.benchmark_group("directory_resolution");

    for levels in NESTING_LEVELS {
        let temp = TempDir::new().expect("create a temporary directory");
        let leaf = tree::deep_nesting(temp.path(), levels)
            .expect("build the nested tree")
            .remove(0);

        group.bench_function(BenchmarkId::new("depth", levels), |b| {
            b.iter_batched(
                || RootResolver::from_config(&config),
                |resolver| {
                    black_box(
                        resolver
                            .resolve_directory(&leaf)
                            .expect("resolve the directory"),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }

    // A vendored manifest is the case where coverage folding decides the answer
    // instead of the first marker found.
    let temp = TempDir::new().expect("create a temporary directory");
    let repo = tree::nested_projects(temp.path(), VENDORED_REPOS)
        .expect("build the vendored tree")
        .remove(0);
    let vendored = repo.join("vendor/left/src");

    group.bench_function("vendored", |b| {
        b.iter_batched(
            || RootResolver::from_config(&config),
            |resolver| {
                black_box(
                    resolver
                        .resolve_directory(&vendored)
                        .expect("resolve the directory"),
                )
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn defaults() -> Config {
    Config::defaults().expect("the built-in configuration parses")
}

fn resolve_all(resolver: &RootResolver, markers: &[PathBuf]) -> Vec<PathBuf> {
    markers
        .iter()
        .map(|dir| {
            resolver
                .resolve(dir, "Cargo.toml")
                .expect("resolve a marker")
        })
        .collect()
}
