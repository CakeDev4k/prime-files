//! A/B timing of a whole-directory listing on real folders, alternating the
//! listing as it was before the 04/11/2026 performance pass (kept literally in
//! `legacy`) with the current `list_dir_sync`.
//!
//! criterion's `benches/listing.rs` measures temp folders of empty files on
//! one drive; this one measures the folders the explorer actually opens
//! (System32, WinSxS, a Google Drive mount), where what each entry costs on
//! top of the directory read is the question.
//!
//! ```text
//! cargo run --release -p nohrs-services --example listing_ab -- <dir> [<dir>…]
//! ```
//!
//! Before measuring, both sides are listed once and compared entry by entry,
//! and the run refuses to time a folder whose listings differ — a faster
//! listing that answers something else is not a result.

#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#![allow(missing_docs)]

use nohrs_services::fs::listing::{FileEntryDto, ListParams, list_dir_sync};
use std::time::Instant;

const ROUNDS: usize = 7;

mod legacy {
    //! `list_dir_impl` as it was before the pass, unchanged but for the
    //! error type: one `symlink_metadata` per entry, re-opening by path what
    //! `read_dir` had already read, and the sort key recomputed on every
    //! comparison.
    use nohrs_services::fs::listing::FileEntryDto;
    use std::fs;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    pub fn list(path: &str, limit: usize) -> std::io::Result<Vec<FileEntryDto>> {
        let dir = std::path::Path::new(path);
        let mut names: Vec<(String, PathBuf)> = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            names.push((file_name, entry.path()));
        }
        names.sort_by_key(|a| a.0.to_lowercase());
        let end = limit.min(names.len());
        let mut entries = Vec::with_capacity(end);
        for (name, path) in names[..end].iter() {
            let md = fs::symlink_metadata(path);
            let (kind, size, modified) = match md {
                Ok(md) => {
                    let modified = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if md.file_type().is_dir() {
                        ("dir".to_string(), 0, modified)
                    } else if md.file_type().is_file() {
                        ("file".to_string(), md.len(), modified)
                    } else if md.file_type().is_symlink() {
                        ("symlink".to_string(), 0, modified)
                    } else {
                        ("other".to_string(), 0, modified)
                    }
                }
                Err(_) => ("unknown".to_string(), 0, 0),
            };
            entries.push(FileEntryDto {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                kind,
                size,
                modified,
            });
        }
        Ok(entries)
    }
}

fn current(path: &str, limit: usize) -> Vec<FileEntryDto> {
    list_dir_sync(ListParams {
        path,
        limit,
        cursor: None,
    })
    .expect("list_dir_sync")
    .entries
}

// The same entries with the same facts, in whatever order: since 04/11/2026 a
// whole listing comes in the filesystem's order and its caller sorts it
// (`listing.rs`, passada 8 of `docs/PERFORMANCE.md`).
fn same(a: &[FileEntryDto], b: &[FileEntryDto]) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{} entries against {}", a.len(), b.len()));
    }
    let by_path = |entries: &[FileEntryDto]| {
        let mut sorted = entries.to_vec();
        sorted.sort_by(|x, y| x.path.cmp(&y.path));
        sorted
    };
    let (a, b) = (by_path(a), by_path(b));
    for (x, y) in a.iter().zip(&b) {
        if (&x.name, &x.path, &x.kind, x.size, x.modified)
            != (&y.name, &y.path, &y.kind, y.size, y.modified)
        {
            return Err(format!("{x:?} against {y:?}"));
        }
    }
    Ok(())
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let folders: Vec<String> = std::env::args().skip(1).collect();
    for folder in &folders {
        let before = legacy::list(folder, usize::MAX).expect("legacy listing");
        let after = current(folder, usize::MAX);
        if let Err(difference) = same(&before, &after) {
            println!("{folder}: listings differ, not timed: {difference}");
            continue;
        }

        let (mut legacy_page, mut legacy_all, mut current_all) = (vec![], vec![], vec![]);
        for _ in 0..ROUNDS {
            // Alternated inside each round, so a drive warming up or the
            // machine getting busier lands on both sides alike.
            let start = Instant::now();
            std::hint::black_box(legacy::list(folder, 1000).expect("legacy"));
            legacy_page.push(start.elapsed().as_secs_f64() * 1e3);

            let start = Instant::now();
            std::hint::black_box(legacy::list(folder, usize::MAX).expect("legacy"));
            legacy_all.push(start.elapsed().as_secs_f64() * 1e3);

            let start = Instant::now();
            std::hint::black_box(current(folder, usize::MAX));
            current_all.push(start.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{folder}: {} entries | before, first 1000: {:.1} ms | before, all: {:.1} ms | now, all: {:.1} ms  (median of {ROUNDS})",
            after.len(),
            median(legacy_page),
            median(legacy_all),
            median(current_all),
        );
    }
}
