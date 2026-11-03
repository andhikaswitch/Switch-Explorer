//! Turning a paste or a drop into an [`Op`], with the user's conflict decisions (FEATURES.md
//! F-OPS-05). Conflicts are found before anything runs; nothing is overwritten without a
//! decision, and Replace recycles the old item so it can be restored.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::name;
use crate::op::{Op, Pair};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// An item that already exists at the destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub source: PathBuf,
    pub target: PathBuf,
    pub source_size: u64,
    pub target_size: u64,
    pub source_modified: Option<SystemTime>,
    pub target_modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Replace,
    Skip,
    /// Copy under a new name: `name (2).ext`.
    KeepBoth,
    /// Replace only when the source is newer; skip otherwise.
    ReplaceIfNewer,
}

/// Where each source would land in `dest` and which of those collide.
pub fn conflicts(sources: &[PathBuf], dest: &Path) -> Vec<Conflict> {
    sources
        .iter()
        .filter_map(|source| {
            let target = dest.join(source.file_name()?);
            if source == &target {
                return None; // Pasting into the same folder: always "Keep both".
            }
            let target_meta = std::fs::metadata(&target).ok()?;
            let source_meta = std::fs::metadata(source).ok();
            Some(Conflict {
                source: source.clone(),
                target,
                source_size: source_meta.as_ref().map_or(0, std::fs::Metadata::len),
                target_size: target_meta.len(),
                source_modified: source_meta.and_then(|m| m.modified().ok()),
                target_modified: target_meta.modified().ok(),
            })
        })
        .collect()
}

/// The operation for pasting `sources` into `dest`. `decide` is asked once per conflict.
/// Pasting a copy into its own folder makes `name (2)` like Explorer; moving into the same
/// folder does nothing.
pub fn paste(
    sources: &[PathBuf],
    dest: &Path,
    mode: Mode,
    mut decide: impl FnMut(&Conflict) -> Resolution,
) -> Option<Op> {
    let found = conflicts(sources, dest);
    let mut pairs = Vec::new();
    let mut replaced = Vec::new();
    for source in sources {
        let Some(file_name) = source.file_name() else {
            continue;
        };
        let target = dest.join(file_name);
        if source == &target {
            if mode == Mode::Move {
                continue;
            }
            let name = name::keep_both(dest, &file_name.to_string_lossy(), Path::exists);
            pairs.push(Pair {
                from: source.clone(),
                to: dest.join(name),
            });
            continue;
        }
        let Some(conflict) = found.iter().find(|c| &c.source == source) else {
            pairs.push(Pair {
                from: source.clone(),
                to: target,
            });
            continue;
        };
        let resolution = match decide(conflict) {
            Resolution::ReplaceIfNewer => {
                match (conflict.source_modified, conflict.target_modified) {
                    (Some(s), Some(t)) if s > t => Resolution::Replace,
                    _ => Resolution::Skip,
                }
            }
            other => other,
        };
        match resolution {
            Resolution::Skip | Resolution::ReplaceIfNewer => {}
            Resolution::Replace => {
                replaced.push(target.clone());
                pairs.push(Pair {
                    from: source.clone(),
                    to: target,
                });
            }
            Resolution::KeepBoth => {
                let taken: Vec<PathBuf> = pairs.iter().map(|p| p.to.clone()).collect();
                let name = name::keep_both(dest, &file_name.to_string_lossy(), |p| {
                    p.exists() || taken.iter().any(|t| t == p)
                });
                pairs.push(Pair {
                    from: source.clone(),
                    to: dest.join(name),
                });
            }
        }
    }
    if pairs.is_empty() {
        return None;
    }
    Some(match mode {
        Mode::Copy => Op::Copy {
            pairs,
            replaced,
            replaced_slots: Vec::new(),
        },
        Mode::Move => Op::Move {
            pairs,
            replaced,
            replaced_slots: Vec::new(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolutions() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = (root.path().join("src"), root.path().join("dst"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            fs::write(src.join(name), b"new").unwrap();
        }
        for name in ["a.txt", "b.txt", "c.txt"] {
            fs::write(dst.join(name), b"old").unwrap();
        }
        let sources: Vec<PathBuf> = ["a.txt", "b.txt", "c.txt", "d.txt"]
            .iter()
            .map(|n| src.join(n))
            .collect();
        assert_eq!(conflicts(&sources, &dst).len(), 3);
        let op = paste(&sources, &dst, Mode::Copy, |c| {
            match c.source.file_name().unwrap().to_str().unwrap() {
                "a.txt" => Resolution::Replace,
                "b.txt" => Resolution::Skip,
                _ => Resolution::KeepBoth,
            }
        })
        .unwrap();
        let Op::Copy {
            pairs, replaced, ..
        } = op
        else {
            panic!("copy expected");
        };
        let targets: Vec<_> = pairs
            .iter()
            .map(|p| p.to.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(targets, ["a.txt", "c (2).txt", "d.txt"]);
        assert_eq!(replaced, [dst.join("a.txt")]);
    }

    #[test]
    fn pasting_into_the_same_folder() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a.txt");
        fs::write(&file, b"x").unwrap();
        let sources = vec![file];
        let copy = paste(&sources, root.path(), Mode::Copy, |_| Resolution::Skip).unwrap();
        assert!(matches!(&copy, Op::Copy { pairs, .. } if pairs[0].to.ends_with("a (2).txt")));
        assert_eq!(
            paste(&sources, root.path(), Mode::Move, |_| Resolution::Skip),
            None
        );
    }
}
