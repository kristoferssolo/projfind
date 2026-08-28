use crate::common::tree;
use color_eyre::eyre::Result;
use criterion::{BenchmarkId, Criterion, Throughput};
use mekle::{config::Config, finder::ProjectFinder};
use std::{hint::black_box, path::Path, time::Duration};
use tempfile::TempDir;

type Shape = fn(&Path, usize) -> Result<Vec<std::path::PathBuf>>;

const SHAPES: [(&str, Shape); 2] = [
    ("flat_repos", tree::flat_repos),
    ("monorepo", tree::monorepo),
];

const SIZES: [usize; 3] = [16, 128, 512];

const SEARCH_DEPTH: usize = 8;

pub fn benchmark_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("discovery");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));

    for (shape, build) in SHAPES {
        for size in SIZES {
            let temp = TempDir::new().expect("create a temporary directory");
            build(temp.path(), size).expect("build the tree");
            let finder = ProjectFinder::new(config_for(temp.path()));

            group.throughput(Throughput::Elements(size as u64));
            group.bench_function(BenchmarkId::new(shape, size), |b| {
                b.iter(|| black_box(finder.find_projects().expect("search the tree")));
            });
        }
    }

    group.finish();
}

fn config_for(root: &Path) -> Config {
    let mut config = Config::defaults().expect("the built-in configuration parses");
    config.paths = vec![root.to_path_buf()];
    config.depth = SEARCH_DEPTH;
    config
}
