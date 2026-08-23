use crate::common::{
    setup::{BenchParams, init_temp_dir},
    utils::run_binary_with_args,
};
use criterion::{BenchmarkId, Criterion};

pub fn benchmark_basic(c: &mut Criterion) {
    let temp_dir = init_temp_dir().path();

    let params = [
        BenchParams {
            depth: Some(1),
            ..Default::default()
        },
        BenchParams {
            depth: Some(5),
            ..Default::default()
        },
        BenchParams {
            depth: Some(10),
            ..Default::default()
        },
        BenchParams {
            depth: Some(10),
            max_results: Some(10),
            ..Default::default()
        },
    ];

    let mut group = c.benchmark_group("basic_scenarios");

    for (idx, param) in params.iter().enumerate() {
        let id = BenchmarkId::new(format!("with_param_{idx}"), param);

        group.bench_with_input(id, param, |b, param| {
            b.iter(|| run_binary_with_args(temp_dir, param).expect("Failed to run binary"));
        });
    }

    group.finish();
}
