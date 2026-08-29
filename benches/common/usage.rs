//! Recorded-usage fixtures for the history and ranking benchmarks.
//!
//! Scores are scrambled so the sorts under measurement never receive input that
//! is already ordered, and ages cover every frecency bucket.

use color_eyre::eyre::Result;
use mekle::{finder::Project, history::HistoryEntry};
use std::{
    fmt::Write as _,
    fs::write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// One age per frecency bucket: this hour, today, this week, and older.
const AGES: [u64; 4] = [30 * 60, 6 * 3600, 3 * 24 * 3600, 30 * 24 * 3600];

/// The multiplier `History` applies to each of those ages.
const MULTIPLIERS: [f64; AGES.len()] = [4.0, 2.0, 0.5, 0.25];

/// Odd multiplier that spreads consecutive indices across the score range.
const SCRAMBLE: usize = 2_654_435_761;

/// Widest score a scrambled index can produce.
const SCORE_RANGE: usize = 997;

/// Path of the `index`th project that history knows about.
pub fn tracked_path(index: usize) -> PathBuf {
    PathBuf::from(format!("/projects/tracked/repo-{index:05}"))
}

/// Path of the `index`th project that history has never seen.
pub fn untracked_path(index: usize) -> PathBuf {
    PathBuf::from(format!("/projects/untracked/repo-{index:05}"))
}

fn score(index: usize) -> f64 {
    let bucket = index.wrapping_mul(SCRAMBLE) % SCORE_RANGE;
    f64::from(u16::try_from(bucket).expect("a remainder below 997 fits in u16")) + 1.0
}

const fn bucket(index: usize) -> usize {
    index % AGES.len()
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

/// Writes a history file holding `count` entries and returns its path.
pub fn history_file(dir: &Path, count: usize) -> Result<PathBuf> {
    let now = now()?;
    let mut contents = String::from("version = 1\n");
    for index in 0..count {
        write!(
            contents,
            // Fixture paths never contain a quote or a backslash, so they
            // need no TOML escaping.
            "\n[[projects]]\npath = \"{}\"\nscore = {:.1}\nlast_accessed = {}\n",
            tracked_path(index).display(),
            score(index),
            now.saturating_sub(AGES[bucket(index)]),
        )?;
    }

    let path = dir.join("history.toml");
    write(&path, contents)?;
    Ok(path)
}

/// Builds `count` history entries without touching the filesystem.
pub fn entries(count: usize) -> Result<Vec<HistoryEntry>> {
    let now = now()?;
    Ok((0..count)
        .map(|index| {
            let age = AGES[bucket(index)];
            HistoryEntry {
                path: tracked_path(index),
                score: score(index),
                frecency: score(index) * MULTIPLIERS[bucket(index)],
                last_used: Duration::from_secs(age),
                last_used_at: now.saturating_sub(age),
                pinned: false,
            }
        })
        .collect())
}

/// Builds `count` discovered projects that [`entries`] covers entry for entry.
pub fn projects(count: usize) -> Vec<Project> {
    build(count, tracked_path)
}

/// Builds `count` discovered projects that no history entry mentions.
pub fn untracked_projects(count: usize) -> Vec<Project> {
    build(count, untracked_path)
}

fn build(count: usize, path: fn(usize) -> PathBuf) -> Vec<Project> {
    (0..count)
        .map(|index| Project {
            path: path(index),
            markers: vec![".git".to_owned(), "Cargo.toml".to_owned()],
        })
        .collect()
}
