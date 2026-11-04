//! Recursive search under a folder (FEATURES.md F-SRCH-02): matches stream out folder by folder
//! while the walk goes on, and it stops as soon as it is cancelled (the query changed).
//!
//! Links (junctions, symlinks) are listed but not followed, so a walk cannot loop or wander to
//! another drive. A folder that cannot be read is counted and skipped; the rest is searched.
//! Blocking: run it on a background thread.

use std::ffi::OsString;
use std::ops::ControlFlow;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sx_core::EntryKind;
use sx_win::RawEntry;

use crate::query::{Candidate, Query};

/// How a walk went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub folders: usize,
    pub entries: usize,
    pub hits: usize,
    /// Folders that could not be read (access denied, gone meanwhile).
    pub skipped: usize,
    pub cancelled: bool,
}

/// Options of a walk: which items are seen, by the same rules as a folder listing.
#[derive(Debug, Clone, Copy, Default)]
pub struct WalkOptions {
    /// Hidden items (`view.show_hidden`).
    pub show_hidden: bool,
    /// Protected OS items, hidden and system (`view.show_system`, with `show_hidden`).
    pub show_system: bool,
}

impl WalkOptions {
    fn shows(self, attrs: u32) -> bool {
        if !self.show_hidden && attrs & FILE_ATTRIBUTE_HIDDEN != 0 {
            return false;
        }
        let protected = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM;
        attrs & protected != protected || self.show_system
    }
}

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

/// Searches `root` and every folder below it, depth first, handing the matching entries of
/// each folder to `sink` together with that folder (an entry's path is the folder joined with
/// its exact name). `now` is the current FILETIME (for `modified:`). `sink` returning `Break`,
/// or `cancel` being set, stops the walk.
pub fn search(
    root: &Path,
    query: &Query,
    options: WalkOptions,
    now: i64,
    cancel: &AtomicBool,
    mut sink: impl FnMut(&Path, &[RawEntry]) -> ControlFlow<()>,
) -> Stats {
    let mut stats = Stats::default();
    let mut pending = vec![root.to_path_buf()];
    let mut batch: Vec<RawEntry> = Vec::new();
    let mut stop = false;
    while let Some(dir) = pending.pop() {
        if cancel.load(Ordering::Relaxed) || stop {
            stats.cancelled = true;
            break;
        }
        stats.folders += 1;
        let mut children = Vec::new();
        let listed = sx_win::enumerate(&dir, |entries| {
            for entry in entries {
                stats.entries += 1;
                if !options.shows(entry.attrs) {
                    continue;
                }
                if entry.kind == EntryKind::Dir {
                    // Lossy names are matched as shown, but the walk goes on by the exact name.
                    children.push(match &entry.exact {
                        Some(exact) => dir.join(OsString::from_wide(exact)),
                        None => dir.join(&entry.name),
                    });
                }
                let candidate = Candidate {
                    name: &entry.name,
                    is_folder: entry.kind.is_container(),
                    size: entry.size,
                    modified: entry.modified,
                };
                if query.matches(&candidate, now) {
                    stats.hits += 1;
                    batch.push(entry.clone());
                }
            }
            // Hand out what this chunk found, so results show while the walk goes on.
            if !batch.is_empty() {
                if sink(&dir, &batch).is_break() {
                    stop = true;
                }
                batch.clear();
            }
            if stop || cancel.load(Ordering::Relaxed) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        if listed.is_err() {
            stats.skipped += 1;
        }
        // Depth first in listing order: the last child pushed is walked first, so push them
        // reversed to walk them in name order.
        pending.extend(children.into_iter().rev());
    }
    stats.cancelled |= cancel.load(Ordering::Relaxed) || stop;
    stats
}

/// The path of an entry handed to the sink.
pub fn entry_path(dir: &Path, entry: &RawEntry) -> PathBuf {
    match &entry.exact {
        Some(exact) => dir.join(OsString::from_wide(exact)),
        None => dir.join(&entry.name),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs").join("2026")).unwrap();
        fs::create_dir_all(root.join("music")).unwrap();
        fs::write(root.join("readme.txt"), b"x").unwrap();
        fs::write(root.join("docs").join("plan.pdf"), vec![0u8; 4096]).unwrap();
        fs::write(root.join("docs").join("2026").join("report.pdf"), b"x").unwrap();
        fs::write(root.join("music").join("song.mp3"), b"x").unwrap();
        dir
    }

    fn names(root: &Path, query: &str) -> (Vec<String>, Stats) {
        let query = Query::parse(query).unwrap();
        let mut out = Vec::new();
        let stats = search(
            root,
            &query,
            WalkOptions::default(),
            i64::MAX,
            &AtomicBool::new(false),
            |dir, entries| {
                out.extend(entries.iter().map(|entry| {
                    entry_path(dir, entry)
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                }));
                ControlFlow::Continue(())
            },
        );
        out.sort();
        (out, stats)
    }

    #[test]
    fn finds_matches_at_every_depth() {
        let dir = tree();
        let (found, stats) = names(dir.path(), "ext:pdf");
        assert_eq!(found, ["docs/2026/report.pdf", "docs/plan.pdf"]);
        assert_eq!(stats.folders, 4);
        assert_eq!(stats.hits, 2);
        assert!(!stats.cancelled);
        let (found, _) = names(dir.path(), "kind:folder");
        assert_eq!(found, ["docs", "docs/2026", "music"]);
        let (found, _) = names(dir.path(), "size:>1kb");
        assert_eq!(found, ["docs/plan.pdf"]);
    }

    #[test]
    fn each_batch_comes_with_its_folder() {
        let dir = tree();
        let query = Query::parse("ext:pdf").unwrap();
        let mut batches = Vec::new();
        search(
            dir.path(),
            &query,
            WalkOptions::default(),
            i64::MAX,
            &AtomicBool::new(false),
            |folder, entries| {
                let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
                batches.push((folder.to_path_buf(), names));
                ControlFlow::Continue(())
            },
        );
        batches.sort();
        assert_eq!(
            batches,
            [
                (dir.path().join("docs"), vec!["plan.pdf".to_string()]),
                (
                    dir.path().join("docs").join("2026"),
                    vec!["report.pdf".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn cancel_and_break_stop_the_walk() {
        let dir = tree();
        let query = Query::parse("*").unwrap();
        let cancel = AtomicBool::new(true);
        let stats = search(
            dir.path(),
            &query,
            WalkOptions::default(),
            i64::MAX,
            &cancel,
            |_, _| ControlFlow::Continue(()),
        );
        assert!(stats.cancelled && stats.folders == 0);
        let mut batches = 0;
        let stats = search(
            dir.path(),
            &query,
            WalkOptions::default(),
            i64::MAX,
            &AtomicBool::new(false),
            |_, _| {
                batches += 1;
                ControlFlow::Break(())
            },
        );
        assert!(stats.cancelled);
        assert_eq!(batches, 1);
    }

    #[test]
    fn hidden_and_system_follow_the_listing_rules() {
        let plain = WalkOptions::default();
        let hidden = WalkOptions {
            show_hidden: true,
            show_system: false,
        };
        let all = WalkOptions {
            show_hidden: true,
            show_system: true,
        };
        let (h, s) = (FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM);
        assert!(plain.shows(0) && plain.shows(s));
        assert!(!plain.shows(h) && !plain.shows(h | s));
        assert!(hidden.shows(h) && !hidden.shows(h | s));
        assert!(all.shows(h | s));
    }

    #[test]
    fn a_missing_root_is_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let (found, stats) = names(&dir.path().join("nope"), "*");
        assert_eq!(found, Vec::<String>::new());
        assert_eq!(stats.skipped, 1);
    }
}
