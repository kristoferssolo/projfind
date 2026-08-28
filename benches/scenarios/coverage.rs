//! [`is_covered`], the ancestor walk that decides whether a candidate is
//! already accounted for by a known project.
//!
//! Cost tracks the distance to the covering ancestor, not the size of the
//! candidate list, so depth is the parameter. A miss is the worst case: it
//! walks every ancestor up to the filesystem root before giving up.

use criterion::{BenchmarkId, Criterion};
use mekle::finder::is_covered;
use std::{
    collections::HashSet,
    hint::black_box,
    path::{Path, PathBuf},
};

/// A direct child is a distinct project, so a hit needs at least two levels.
const DEPTHS: [usize; 3] = [2, 6, 16];

/// Projects already accepted when the candidate is tested.
const KNOWN_ROOTS: usize = 1024;

pub fn benchmark_coverage(c: &mut Criterion) {
    let mut known = (0..KNOWN_ROOTS)
        .map(|index| PathBuf::from(format!("/repos/workspace/crates/member-{index:05}")))
        .collect::<HashSet<_>>();
    let covering = PathBuf::from("/repos/workspace/crates/member-00000");
    let stranger = PathBuf::from("/elsewhere/detached/tree");

    let mut group = c.benchmark_group("coverage");

    for depth in DEPTHS {
        let hit = descend(&covering, depth);
        let miss = descend(&stranger, depth);

        group.bench_function(BenchmarkId::new("hit", depth), |b| {
            b.iter(|| black_box(is_covered(black_box(&hit), &known)));
        });
        group.bench_function(BenchmarkId::new("miss", depth), |b| {
            b.iter(|| black_box(is_covered(black_box(&miss), &known)));
        });
    }

    // The fast path: the candidate is itself a known project.
    known.insert(covering.clone());
    group.bench_function("exact", |b| {
        b.iter(|| black_box(is_covered(black_box(&covering), &known)));
    });

    group.finish();
}

fn descend(root: &Path, depth: usize) -> PathBuf {
    (0..depth).fold(root.to_path_buf(), |dir, level| {
        dir.join(format!("level-{level:02}"))
    })
}
