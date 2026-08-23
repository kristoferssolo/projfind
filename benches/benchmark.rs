mod common;
mod scenarios;

use common::setup::init_temp_dir;
use criterion::{Criterion, criterion_group, criterion_main};
use scenarios::{
    basic::benchmark_basic, edge_cases::benchmark_edge_cases,
    specific::benchmark_specific_scenarios,
};
use std::time::Duration;

criterion_group!(
    name = benches;
    config = {
        let c = Criterion::default()
            .sample_size(10)
            .measurement_time(Duration::from_secs(30));
        init_temp_dir();
        c
    };
    targets = benchmark_basic, benchmark_edge_cases, benchmark_specific_scenarios
);
criterion_main!(benches);
