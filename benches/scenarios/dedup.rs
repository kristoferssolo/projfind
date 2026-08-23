//! Folding resolved roots into the result set.
//!
//! Pure path arithmetic, so no tree is built here. The point is the shape of
//! the growth: coverage is an ancestor lookup rather than a comparison against
//! every project found so far, and this is what would catch that turning
//! quadratic.

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

/// The fold `find_projects` runs: candidates arrive shallowest first, and each
/// either joins the set or is absorbed by a project already in it.
fn absorb(candidates: &[PathBuf]) -> HashSet<PathBuf> {
    let mut projects = HashSet::with_capacity(candidates.len());

    for candidate in candidates {
        if !is_covered(candidate, &projects) {
            projects.insert(candidate.clone());
        }
    }

    projects
}

/// Half the candidates are members that survive, half are directories nested
/// under one of them and get absorbed, so both branches of the check are paid
/// for. Sorted, as `find_projects` sorts before folding.
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
