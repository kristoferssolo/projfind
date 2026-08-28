//! End-to-end discovery, measured against the bare walk it is built on.
//!
//! The two groups run the same trees at the same sizes. `scan` is the parallel
//! directory walk and its classification alone, so `discovery` minus `scan` is
//! what mekle spends on root resolution and folding candidates into projects.
//!
//! Discovery builds a fresh [`ProjectFinder`] per iteration. A reused finder
//! keeps its resolver caches warm, which no real invocation ever gets.

use crate::common::tree;
use color_eyre::eyre::Result;
use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput, measurement::WallTime,
};
use mekle::{config::Config, finder::ProjectFinder, scan::scan_directories};
use std::{
    hint::black_box,
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;

type Shape = fn(&Path, usize) -> Result<Vec<PathBuf>>;

const SHAPES: [(&str, Shape); 4] = [
    ("flat_repos", tree::flat_repos),
    ("monorepo", tree::monorepo),
    ("haystack", tree::haystack),
    ("nested_projects", tree::nested_projects),
];

const SIZES: [usize; 2] = [64, 512];

const SEARCH_DEPTH: usize = 8;

/// Every iteration walks a whole tree, so samples are few and each one is cheap
/// to repeat; the defaults would spend minutes here for no extra resolution.
fn tune(group: &mut BenchmarkGroup<'_, WallTime>) {
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));
}

pub fn benchmark_scan(c: &mut Criterion) {
    let marker_files = defaults().marker_files;

    let mut group = c.benchmark_group("scan");
    tune(&mut group);

    for (shape, build) in SHAPES {
        for size in SIZES {
            let temp = TempDir::new().expect("create a temporary directory");
            build(temp.path(), size).expect("build the tree");
            let dirs = vec![temp.path().to_path_buf()];

            group.throughput(Throughput::Elements(size as u64));
            group.bench_function(BenchmarkId::new(shape, size), |b| {
                b.iter(|| {
                    let scan = scan_directories(&dirs, &marker_files, SEARCH_DEPTH);
                    black_box((scan.git_repos, scan.marker_files))
                });
            });
        }
    }

    group.finish();
}

pub fn benchmark_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("discovery");
    tune(&mut group);

    for (shape, build) in SHAPES {
        for size in SIZES {
            let temp = TempDir::new().expect("create a temporary directory");
            build(temp.path(), size).expect("build the tree");
            let config = config_for(temp.path());

            group.throughput(Throughput::Elements(size as u64));
            group.bench_function(BenchmarkId::new(shape, size), |b| {
                b.iter_batched(
                    || ProjectFinder::new(config.clone()),
                    |finder| black_box(finder.find_project_details().expect("search the tree")),
                    BatchSize::PerIteration,
                );
            });
        }
    }

    group.finish();
}

fn defaults() -> Config {
    Config::defaults().expect("the built-in configuration parses")
}

fn config_for(root: &Path) -> Config {
    let mut config = defaults();
    config.paths = vec![root.to_path_buf()];
    config.depth = SEARCH_DEPTH;
    config
}
