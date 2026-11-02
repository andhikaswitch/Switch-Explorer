//! Runs an [`Op`] (blocking; call it on a worker, never on the UI thread). Progress is reported
//! per item, cancel is checked between items and between files inside folders, and nothing is
//! ever overwritten: Replace decisions were turned into "recycle the old one first" by the plan.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::OpsError;
use crate::op::{Op, Pair};

/// The Recycle Bin, provided by the shell (`sx-shell`) in the app and by a folder in tests.
pub trait Trash: Send + Sync {
    /// Recycles `paths` and returns a stamp taken just before (seconds since the epoch for the
    /// real Recycle Bin), so the same items can be found again by [`Trash::restore`].
    fn recycle(&self, paths: &[PathBuf]) -> Result<u64, OpsError>;
    /// Restores recycled items to these original paths: for each path, the earliest item
    /// recycled at or after `since`, or the most recent one when `since` is `None`. (Undoing a
    /// Replace recycles the copy first; the replaced item is the older of the two.)
    fn restore(&self, paths: &[PathBuf], since: Option<u64>) -> Result<(), OpsError>;
}

/// Progress of a running operation (reported at most once per item).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    /// The top-level item being worked on.
    pub current: Option<PathBuf>,
    pub bytes: u64,
}

/// Cancels a running operation from another thread.
#[derive(Debug, Default)]
pub struct Cancel(AtomicBool);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// What was actually done: the part of the op that completed (for the journal and undo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub done: Op,
    pub cancelled: bool,
}

pub fn run(
    op: &Op,
    trash: &dyn Trash,
    cancel: &Cancel,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Outcome, OpsError> {
    let mut state = Progress {
        total: op.item_count(),
        ..Progress::default()
    };
    run_inner(op, trash, cancel, progress, &mut state)
}

fn run_inner(
    op: &Op,
    trash: &dyn Trash,
    cancel: &Cancel,
    progress: &mut dyn FnMut(&Progress),
    state: &mut Progress,
) -> Result<Outcome, OpsError> {
    let finished = |done: Op| Outcome {
        done,
        cancelled: false,
    };
    match op {
        Op::Rename { from, to } => {
            if to.exists() && !same_item(from, to) {
                return Err(OpsError::Exists(to.clone()));
            }
            fs::rename(from, to).map_err(|e| OpsError::io(from, e))?;
            Ok(finished(op.clone()))
        }
        Op::NewFolder { path } => {
            fs::create_dir(path).map_err(|e| OpsError::io(path, e))?;
            Ok(finished(op.clone()))
        }
        Op::Recycle { paths, .. } => {
            let at = trash.recycle(paths)?;
            Ok(finished(Op::Recycle {
                paths: paths.clone(),
                at: Some(at),
            }))
        }
        Op::Restore { paths, since } => {
            trash.restore(paths, *since)?;
            Ok(finished(op.clone()))
        }
        Op::Delete { paths } => {
            let mut done = Vec::new();
            for path in paths {
                report(progress, state, path);
                if cancel.is_cancelled() {
                    return Ok(Outcome {
                        done: Op::Delete { paths: done },
                        cancelled: true,
                    });
                }
                remove(path)?;
                done.push(path.clone());
            }
            Ok(finished(Op::Delete { paths: done }))
        }
        Op::Copy {
            pairs, replaced, ..
        }
        | Op::Move {
            pairs, replaced, ..
        } => transfer(
            matches!(op, Op::Move { .. }),
            pairs,
            replaced,
            trash,
            cancel,
            progress,
            state,
        ),
        Op::Batch { ops } => {
            let mut done = Vec::new();
            for step in ops {
                let outcome = run_inner(step, trash, cancel, progress, state)?;
                done.push(outcome.done);
                if outcome.cancelled {
                    return Ok(Outcome {
                        done: Op::Batch { ops: done },
                        cancelled: true,
                    });
                }
            }
            Ok(finished(Op::Batch { ops: done }))
        }
    }
}

/// Copies or moves `pairs`; `replaced` targets are recycled first (the user chose Replace).
#[allow(clippy::too_many_arguments)]
fn transfer(
    moving: bool,
    pairs: &[Pair],
    replaced: &[PathBuf],
    trash: &dyn Trash,
    cancel: &Cancel,
    progress: &mut dyn FnMut(&Progress),
    state: &mut Progress,
) -> Result<Outcome, OpsError> {
    let replaced_at = if replaced.is_empty() {
        None
    } else {
        Some(trash.recycle(replaced)?)
    };
    let rebuild = |pairs| {
        if moving {
            Op::Move {
                pairs,
                replaced: replaced.to_vec(),
                replaced_at,
            }
        } else {
            Op::Copy {
                pairs,
                replaced: replaced.to_vec(),
                replaced_at,
            }
        }
    };
    let mut done = Vec::new();
    for pair in pairs {
        report(progress, state, &pair.from);
        if cancel.is_cancelled() {
            return Ok(Outcome {
                done: rebuild(done),
                cancelled: true,
            });
        }
        check_target(pair)?;
        if moving {
            move_item(pair, cancel, state)?;
        } else {
            copy_tree(&pair.from, &pair.to, cancel, state)?;
        }
        done.push(pair.clone());
    }
    Ok(Outcome {
        done: rebuild(done),
        cancelled: false,
    })
}

fn report(progress: &mut dyn FnMut(&Progress), state: &mut Progress, current: &Path) {
    state.current = Some(current.to_path_buf());
    progress(state);
    state.done += 1;
}

/// Never write over something, and never copy a folder into itself.
fn check_target(pair: &Pair) -> Result<(), OpsError> {
    if pair.to.exists() {
        return Err(OpsError::Exists(pair.to.clone()));
    }
    if pair.from.is_dir() && pair.to.starts_with(&pair.from) {
        return Err(OpsError::IntoItself(pair.from.clone()));
    }
    Ok(())
}

fn same_item(a: &Path, b: &Path) -> bool {
    // A case-only rename (`photo.JPG` → `photo.jpg`) targets the same item.
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn volume(path: &Path) -> Option<String> {
    match path.components().next()? {
        Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_uppercase()),
        _ => None,
    }
}

/// Same volume: a rename. Across volumes: copy, check, then remove the source
/// (`.claude/rules/file-ops-safety.md`).
fn move_item(pair: &Pair, cancel: &Cancel, state: &mut Progress) -> Result<(), OpsError> {
    if volume(&pair.from).is_some() && volume(&pair.from) == volume(&pair.to) {
        return fs::rename(&pair.from, &pair.to).map_err(|e| OpsError::io(&pair.from, e));
    }
    copy_tree(&pair.from, &pair.to, cancel, state)?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    let (source, copied) = (tree_size(&pair.from)?, tree_size(&pair.to)?);
    if source != copied {
        return Err(OpsError::VerifyFailed(pair.to.clone()));
    }
    remove(&pair.from)
}

/// Copies a file or a folder tree. Links inside folders (junctions, symlinks) are not followed:
/// they are skipped, so a copy never walks into another part of the disk.
fn copy_tree(
    from: &Path,
    to: &Path,
    cancel: &Cancel,
    state: &mut Progress,
) -> Result<(), OpsError> {
    let meta = fs::symlink_metadata(from).map_err(|e| OpsError::io(from, e))?;
    if meta.is_file() {
        let bytes = fs::copy(from, to).map_err(|e| OpsError::io(from, e))?;
        state.bytes += bytes;
        return Ok(());
    }
    if !meta.is_dir() {
        return Ok(()); // A link at the top level of a tree walk: skipped.
    }
    fs::create_dir(to).map_err(|e| OpsError::io(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| OpsError::io(from, e))? {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let entry = entry.map_err(|e| OpsError::io(from, e))?;
        let kind = entry
            .file_type()
            .map_err(|e| OpsError::io(&entry.path(), e))?;
        if kind.is_symlink() {
            continue;
        }
        copy_tree(&entry.path(), &to.join(entry.file_name()), cancel, state)?;
    }
    Ok(())
}

fn tree_size(path: &Path) -> Result<u64, OpsError> {
    let meta = fs::symlink_metadata(path).map_err(|e| OpsError::io(path, e))?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in fs::read_dir(path).map_err(|e| OpsError::io(path, e))? {
        let entry = entry.map_err(|e| OpsError::io(path, e))?;
        if !entry.file_type().is_ok_and(|t| t.is_symlink()) {
            total += tree_size(&entry.path())?;
        }
    }
    Ok(total)
}

/// Permanent removal. `remove_dir_all` removes links themselves, never their targets.
fn remove(path: &Path) -> Result<(), OpsError> {
    let meta = fs::symlink_metadata(path).map_err(|e| OpsError::io(path, e))?;
    let removed = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(|e| OpsError::io(path, e))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A Recycle Bin in a temp folder: recycled items are moved there and back.
    pub struct FolderTrash {
        pub bin: PathBuf,
        /// (original path, slot in the bin, stamp)
        items: Mutex<Vec<(PathBuf, PathBuf, u64)>>,
    }

    impl FolderTrash {
        pub fn new(bin: PathBuf) -> Self {
            fs::create_dir_all(&bin).unwrap();
            Self {
                bin,
                items: Mutex::default(),
            }
        }
    }

    impl Trash for FolderTrash {
        fn recycle(&self, paths: &[PathBuf]) -> Result<u64, OpsError> {
            let mut items = self.items.lock().unwrap();
            let stamp = items.len() as u64;
            for path in paths {
                let slot = self.bin.join(format!("{}", items.len()));
                fs::rename(path, &slot).map_err(|e| OpsError::io(path, e))?;
                items.push((path.clone(), slot, stamp));
            }
            Ok(stamp)
        }

        fn restore(&self, paths: &[PathBuf], since: Option<u64>) -> Result<(), OpsError> {
            let mut items = self.items.lock().unwrap();
            for path in paths {
                let matching = |(original, _, at): &(PathBuf, PathBuf, u64)| {
                    original == path && since.is_none_or(|s| *at >= s)
                };
                let ix = match since {
                    Some(_) => items.iter().position(matching),
                    None => items.iter().rposition(matching),
                }
                .ok_or_else(|| OpsError::NotInRecycleBin(path.clone()))?;
                let (original, slot, _) = items.remove(ix);
                fs::rename(&slot, &original).map_err(|e| OpsError::io(&slot, e))?;
            }
            Ok(())
        }
    }

    fn quiet() -> impl FnMut(&Progress) {
        |_| {}
    }

    #[test]
    fn copy_move_rename_and_undo() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let (src, dst) = (root.path().join("src"), root.path().join("dst"));
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();
        fs::write(src.join("sub").join("b.txt"), b"world").unwrap();
        fs::create_dir(&dst).unwrap();
        let cancel = Cancel::default();

        let copy = Op::Copy {
            pairs: vec![Pair {
                from: src.clone(),
                to: dst.join("src"),
            }],
            replaced: vec![],
            replaced_at: None,
        };
        let outcome = run(&copy, &trash, &cancel, &mut quiet()).unwrap();
        assert_eq!(
            fs::read(dst.join("src").join("sub").join("b.txt")).unwrap(),
            b"world"
        );
        // Undo recycles the copy; the source is untouched.
        run(
            &outcome.done.inverse().unwrap(),
            &trash,
            &cancel,
            &mut quiet(),
        )
        .unwrap();
        assert!(!dst.join("src").exists() && src.join("a.txt").exists());

        let rename = Op::Rename {
            from: src.join("a.txt"),
            to: src.join("A.txt"),
        };
        run(&rename, &trash, &cancel, &mut quiet()).unwrap();
        let moving = Op::Move {
            pairs: vec![Pair {
                from: src.join("A.txt"),
                to: dst.join("A.txt"),
            }],
            replaced: vec![],
            replaced_at: None,
        };
        let outcome = run(&moving, &trash, &cancel, &mut quiet()).unwrap();
        assert!(dst.join("A.txt").exists());
        run(
            &outcome.done.inverse().unwrap(),
            &trash,
            &cancel,
            &mut quiet(),
        )
        .unwrap();
        assert!(src.join("A.txt").exists() && !dst.join("A.txt").exists());
    }

    #[test]
    fn never_overwrites_and_never_copies_into_itself() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let a = root.path().join("a");
        fs::create_dir(&a).unwrap();
        fs::write(root.path().join("x.txt"), b"1").unwrap();
        fs::write(a.join("x.txt"), b"2").unwrap();
        let cancel = Cancel::default();
        let over = Op::Copy {
            pairs: vec![Pair {
                from: root.path().join("x.txt"),
                to: a.join("x.txt"),
            }],
            replaced: vec![],
            replaced_at: None,
        };
        assert!(matches!(
            run(&over, &trash, &cancel, &mut quiet()),
            Err(OpsError::Exists(_))
        ));
        assert_eq!(fs::read(a.join("x.txt")).unwrap(), b"2");
        let into = Op::Copy {
            pairs: vec![Pair {
                from: a.clone(),
                to: a.join("inner"),
            }],
            replaced: vec![],
            replaced_at: None,
        };
        assert!(matches!(
            run(&into, &trash, &cancel, &mut quiet()),
            Err(OpsError::IntoItself(_))
        ));
    }

    #[test]
    fn replace_recycles_the_old_item_and_undo_brings_it_back() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        fs::write(root.path().join("x.txt"), b"new").unwrap();
        fs::write(dst.join("x.txt"), b"old").unwrap();
        let cancel = Cancel::default();
        let op = Op::Copy {
            pairs: vec![Pair {
                from: root.path().join("x.txt"),
                to: dst.join("x.txt"),
            }],
            replaced: vec![dst.join("x.txt")],
            replaced_at: None,
        };
        let outcome = run(&op, &trash, &cancel, &mut quiet()).unwrap();
        assert_eq!(fs::read(dst.join("x.txt")).unwrap(), b"new");
        run(
            &outcome.done.inverse().unwrap(),
            &trash,
            &cancel,
            &mut quiet(),
        )
        .unwrap();
        assert_eq!(fs::read(dst.join("x.txt")).unwrap(), b"old");
    }

    #[test]
    fn cancel_keeps_what_was_done() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        let pairs: Vec<Pair> = (0..5)
            .map(|i| {
                let from = root.path().join(format!("{i}.txt"));
                fs::write(&from, b"x").unwrap();
                Pair {
                    to: dst.join(format!("{i}.txt")),
                    from,
                }
            })
            .collect();
        let cancel = Cancel::default();
        let mut seen = 0;
        let op = Op::Copy {
            pairs,
            replaced: vec![],
            replaced_at: None,
        };
        let outcome = run(&op, &trash, &cancel, &mut |p: &Progress| {
            seen += 1;
            if p.done == 2 {
                cancel.cancel();
            }
        })
        .unwrap();
        assert!(outcome.cancelled);
        assert_eq!(outcome.done.item_count(), 2, "{seen} reports");
    }
}
