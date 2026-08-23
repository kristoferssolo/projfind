//! End-to-end searches, the number a user actually waits on.
//!
//! Every sample spawns `fd` and walks a real tree, so these are the slowest
//! benchmarks here and the only ones whose result includes the scanner.

use crate::common::tree;
use color_eyre::eyre::Result;
use criterion::{BenchmarkId, Criterion, Throughput};
use projfind::{config::Config, dependencies::Dependencies, finder::ProjectFinder};
use std::{hint::black_box, path::Path, time::Duration};
use tempfile::TempDir;
use tokio::runtime::Runtime;

type Shape = fn(&Path, usize) -> Result<Vec<std::path::PathBuf>>;

/// A monorepo collapses to one result and a flat tree to `count` of them, so
/// the pair brackets the work the resolver has to do after a scan.
const SHAPES: [(&str, Shape); 2] = [
    ("flat_repos", tree::flat_repos),
    ("monorepo", tree::monorepo),
];

const SIZES: [usize; 3] = [16, 128, 512];

/// Deep enough for every shape here, and fixed so the numbers do not move when
/// the shipped default does.
const SEARCH_DEPTH: usize = 8;

pub fn benchmark_discovery(c: &mut Criterion) {
    let deps = Dependencies::check().expect("`fd` has to be on PATH to benchmark discovery");
    let runtime = Runtime::new().expect("build a tokio runtime");

    let mut group = c.benchmark_group("discovery");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));

    for (shape, build) in SHAPES {
        for size in SIZES {
            let temp = TempDir::new().expect("create a temporary directory");
            build(temp.path(), size).expect("build the tree");
            let finder = ProjectFinder::new(config_for(temp.path()), deps.clone());

            group.throughput(Throughput::Elements(size as u64));
            group.bench_function(BenchmarkId::new(shape, size), |b| {
                b.to_async(&runtime).iter(|| async {
                    black_box(finder.find_projects().await.expect("search the tree"))
                });
            });
        }
    }

    group.finish();
}

/// The shipped defaults, pointed at one tree: benchmarking against the real
/// marker list keeps the `fd` pattern the same size as in production.
fn config_for(root: &Path) -> Config {
    let mut config = Config::defaults().expect("the built-in configuration parses");
    config.paths = vec![root.to_path_buf()];
    config.depth = SEARCH_DEPTH;
    config
}
