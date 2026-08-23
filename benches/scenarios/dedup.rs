use criterion::{BenchmarkId, Criterion, Throughput};
use projfind::finder::is_covered;
use std::{collections::HashSet, hint::black_box, path::PathBuf};

const CANDIDATE_COUNTS: [usize; 3] = [64, 1024, 16384];

pub fn benchmark_dedup(c: &mut Criterion) {
    let mut group = c.benchmark_group("dedup");

    for count in CANDIDATE_COUNTS {
        let candidates = candidates(count);

        group.throughput(Throughput::Elements(candidates.len() as u64));
        group.bench_function(BenchmarkId::from_parameter(count), |b| {
            b.iter(|| black_box(absorb(&candidates)));
        });
    }

    group.finish();
}

fn absorb(candidates: &[PathBuf]) -> HashSet<PathBuf> {
    let mut projects = HashSet::with_capacity(candidates.len());

    for candidate in candidates {
        if !is_covered(candidate, &projects) {
            projects.insert(candidate.clone());
        }
    }

    projects
}

fn candidates(count: usize) -> Vec<PathBuf> {
    let mut candidates = (0..count / 2)
        .flat_map(|index| {
            let member = PathBuf::from(format!("/repos/workspace/crates/member-{index:05}"));
            let nested = member.join("vendor/bundled");
            [member, nested]
        })
        .collect::<Vec<_>>();

    candidates.sort_unstable();
    candidates
}
