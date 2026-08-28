//! Joining discovered projects to their history and printing the result.
//!
//! Both steps run on every invocation over the full result set, before
//! `max_results` trims anything. Discovery hands them projects already sorted
//! by path, which is why the two ranking variants differ so much: an untracked
//! set is one long tie on frecency that leaves the input order intact, while a
//! tracked set is a full reorder driven by a map lookup per comparison.

use crate::common::usage;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use mekle::{
    config::OutputFormat,
    finder::Project,
    history::HistoryEntry,
    output::{rank_projects, write_projects},
};
use std::hint::black_box;

const PROJECT_COUNTS: [usize; 3] = [64, 1024, 8192];

const FORMATS: [(&str, OutputFormat); 3] = [
    ("path", OutputFormat::Path),
    ("json", OutputFormat::Json),
    ("null", OutputFormat::Null),
];

/// Rough bytes per record, so the output buffer never grows mid-measurement.
const BYTES_PER_PROJECT: usize = 256;

pub fn benchmark_ranking(c: &mut Criterion) {
    let mut group = c.benchmark_group("ranking");

    for count in PROJECT_COUNTS {
        let entries = usage::entries(count).expect("build history entries");
        let tracked = usage::projects(count);
        let untracked = usage::untracked_projects(count);

        group.throughput(Throughput::Elements(count as u64));

        // Every project has a distinct frecency, so the sort is a real reorder.
        bench_rank(&mut group, "tracked", count, &tracked, &entries);
        // Nothing matches: the tie-break on path keeps the incoming order.
        bench_rank(&mut group, "untracked", count, &untracked, &entries);
    }

    group.finish();
}

pub fn benchmark_output(c: &mut Criterion) {
    let mut group = c.benchmark_group("output");

    for count in PROJECT_COUNTS {
        let entries = usage::entries(count).expect("build history entries");
        let results = rank_projects(usage::projects(count), &entries);

        group.throughput(Throughput::Elements(count as u64));
        for (name, format) in FORMATS {
            group.bench_function(BenchmarkId::new(name, count), |b| {
                b.iter_batched(
                    || Vec::<u8>::with_capacity(count * BYTES_PER_PROJECT),
                    |mut sink| {
                        write_projects(&mut sink, &results, format, None)
                            .expect("write the projects");
                        black_box(sink)
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }

    group.finish();
}

fn bench_rank(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    count: usize,
    projects: &[Project],
    entries: &[HistoryEntry],
) {
    group.bench_function(BenchmarkId::new(name, count), |b| {
        b.iter_batched(
            || projects.to_vec(),
            |projects| black_box(rank_projects(projects, entries)),
            BatchSize::LargeInput,
        );
    });
}
