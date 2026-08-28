//! Reading, scoring and rewriting the frecency database.
//!
//! Every invocation of mekle opens this file, and every recorded jump rewrites
//! it in full, so its cost scales with how long a user has been using the tool.
//!
//! `record` writes through a temporary directory. When `TMPDIR` is a tmpfs the
//! two `fsync` calls in `History::save` are free, so treat that number as a
//! floor rather than the cost on a real data directory.

use crate::common::usage;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use mekle::history::History;
use std::{hint::black_box, path::Path, time::Duration};
use tempfile::TempDir;

const ENTRY_COUNTS: [usize; 3] = [64, 1024, 8192];

pub fn benchmark_history(c: &mut Criterion) {
    let mut group = c.benchmark_group("history");

    for count in ENTRY_COUNTS {
        let temp = TempDir::new().expect("create a temporary directory");
        let path = usage::history_file(temp.path(), count).expect("write the history file");

        group.throughput(Throughput::Elements(count as u64));

        group.bench_function(BenchmarkId::new("open", count), |b| {
            b.iter(|| black_box(open(&path)));
        });

        let history = open(&path);
        group.bench_function(BenchmarkId::new("entries", count), |b| {
            b.iter(|| black_box(history.entries().expect("score the entries")));
        });
    }

    group.finish();
}

/// A recorded jump: re-read the database, bump one score, write it back.
pub fn benchmark_record(c: &mut Criterion) {
    let mut group = c.benchmark_group("record");
    group
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));

    for count in ENTRY_COUNTS {
        let temp = TempDir::new().expect("create a temporary directory");
        let path = usage::history_file(temp.path(), count).expect("write the history file");
        let project = usage::tracked_path(0);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::from_parameter(count), |b| {
            b.iter_batched(
                || open(&path),
                |mut history| {
                    history.record(&project).expect("record the visit");
                    black_box(history)
                },
                BatchSize::PerIteration,
            );
        });
    }

    group.finish();
}

fn open(path: &Path) -> History {
    History::open(path).expect("open the history file")
}
