mod common;
mod scenarios;

use criterion::{Criterion, criterion_group, criterion_main};
use scenarios::{
    dedup::benchmark_dedup,
    discovery::benchmark_discovery,
    resolution::{benchmark_deep_ascent, benchmark_root_resolution},
};

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = benchmark_discovery,
        benchmark_root_resolution,
        benchmark_deep_ascent,
        benchmark_dedup,
);
criterion_main!(benches);
