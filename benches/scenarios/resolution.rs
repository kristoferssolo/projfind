//! Resolving markers to the root they belong to.
//!
//! This is the part of a search that is projfind's own work rather than `fd`'s,
//! and the part that grows with the size of a workspace.

use crate::common::tree;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use projfind::{config::Config, finder::root::RootResolver};
use std::{hint::black_box, path::PathBuf};
use tempfile::TempDir;

const MEMBER_COUNTS: [usize; 3] = [16, 128, 1024];
const NESTING_LEVELS: [usize; 3] = [4, 16, 64];

/// A workspace hands the resolver thousands of members sharing one root. Cold
/// and warm are reported side by side because the gap between them *is* the
/// memoisation: if it ever closes, the caches have stopped working.
pub fn benchmark_root_resolution(c: &mut Criterion) {
    let workspace_files = Config::defaults()
        .expect("the built-in configuration parses")
        .workspace_files;

    let mut group = c.benchmark_group("root_resolution");

    for members in MEMBER_COUNTS {
        let temp = TempDir::new().expect("create a temporary directory");
        let markers = tree::monorepo(temp.path(), members).expect("build the monorepo");

        group.throughput(Throughput::Elements(members as u64));

        group.bench_function(BenchmarkId::new("cold_cache", members), |b| {
            b.iter_batched(
                || RootResolver::new(workspace_files.clone()),
                |resolver| black_box(resolve_all(&resolver, &markers)),
                BatchSize::SmallInput,
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

/// One marker, many ancestors: the cost of a single cache miss, which grows
/// with how far the marker sits below its root.
pub fn benchmark_deep_ascent(c: &mut Criterion) {
    let workspace_files = Config::defaults()
        .expect("the built-in configuration parses")
        .workspace_files;

    let mut group = c.benchmark_group("deep_ascent");

    for levels in NESTING_LEVELS {
        let temp = TempDir::new().expect("create a temporary directory");
        let markers = tree::deep_nesting(temp.path(), levels).expect("build the nested tree");

        group.throughput(Throughput::Elements(levels as u64));
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
