//! Benchmarks for mekle, grouped by the stage of a run they cover.
//!
//! Run one stage with `just bench <group>`, for example `just bench ranking`.
//! The `scan` and `discovery` groups need `fd` on `PATH`.

mod common;
mod scenarios;

use criterion::{Criterion, criterion_group, criterion_main};
use scenarios::{
    coverage::benchmark_coverage,
    discovery::{benchmark_discovery, benchmark_scan},
    history::{benchmark_history, benchmark_record},
    ranking::{benchmark_output, benchmark_ranking},
    resolution::{
        benchmark_deep_ascent, benchmark_directory_resolution, benchmark_root_resolution,
    },
    startup::benchmark_startup,
};

criterion_group!(
    name = search;
    config = Criterion::default();
    targets = benchmark_scan, benchmark_discovery,
);
criterion_group!(
    name = roots;
    config = Criterion::default();
    targets = benchmark_root_resolution,
        benchmark_deep_ascent,
        benchmark_directory_resolution,
        benchmark_coverage,
);
criterion_group!(
    name = usage;
    config = Criterion::default();
    targets = benchmark_history, benchmark_record, benchmark_ranking, benchmark_output,
);
criterion_group!(
    name = startup;
    config = Criterion::default();
    targets = benchmark_startup,
);
criterion_main!(search, roots, usage, startup);
