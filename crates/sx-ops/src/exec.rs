//! Runs an [`Op`] (blocking; call it on a worker, never on the UI thread). Progress is reported
//! per item, cancel is checked between items and between files inside folders, and nothing is
//! ever overwritten: Replace decisions were turned into "recycle the old one first" by the plan.
//!
//! An operation that stops early (cancel, an error, an item the Recycle Bin refused) still
//! reports what it did, item by item, so the journal and undo cover exactly that part.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::OpsError;
use crate::op::{Binned, Op, Pair};

/// What the Recycle Bin did with a request, item by item.
#[derive(Debug, Default)]
pub struct Recycled {
    /// Items now in the Recycle Bin, in request order.
    pub binned: Vec<Binned>,
    /// Items the Recycle Bin could not take (too large, a network drive) that were deleted
    /// permanently after the user agreed in the shell's warning.
    pub deleted: Vec<PathBuf>,
    /// The user declined that warning, so the remaining items stayed where they were.
    pub cancelled: bool,
    /// Why the remaining items stayed where they were.
    pub error: Option<OpsError>,
}

/// What a restore did, item by item.
#[derive(Debug, Default)]
pub struct Restored {
    /// Each item that is back, with the path it came back at.
    pub done: Vec<(Binned, PathBuf)>,
    /// Why the remaining items are still in the Recycle Bin.
    pub error: Option<OpsError>,
}

/// What a purge did.
#[derive(Debug, Default)]
pub struct Purged {
    /// The items deleted for good.
    pub done: Vec<Binned>,
    /// Why the remaining items are still in the Recycle Bin.
    pub error: Option<OpsError>,
}

/// The Recycle Bin, provided by the shell (`sx-shell`) in the app and by a folder in tests.
pub trait Trash: Send + Sync {
    /// Recycles `paths` as far as it gets: an item that cannot go does not hide what happened
    /// to the others.
    fn recycle(&self, paths: &[PathBuf]) -> Recycled;
    /// Restores these items from their slots. An item whose original name is taken again comes
    /// back as "name (2)" instead of replacing anything; a missing slot does not stop the rest.
    fn restore(&self, items: &[Binned]) -> Restored;
    /// Deletes these items from the Recycle Bin for good: only the items that are still in their
    /// slots and still are what was listed (an emptied and reused slot is left alone).
    fn purge(&self, items: &[Binned]) -> Purged;
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

/// What was actually done.
#[derive(Debug)]
pub struct Outcome {
    /// The part of the op that completed (for the journal and undo).
    pub done: Op,
    pub cancelled: bool,
    /// Why it stopped before the end; `done` still holds what completed before.
    pub error: Option<OpsError>,
    /// Items deleted permanently instead of recycled (the user agreed in the shell's warning).
    pub deleted: Vec<PathBuf>,
}

impl Outcome {
    fn finished(done: Op) -> Self {
        Self {
            done,
            cancelled: false,
            error: None,
            deleted: Vec::new(),
        }
    }

    fn failed(done: Op, error: OpsError) -> Self {
        Self {
            error: Some(error),
            ..Self::finished(done)
        }
    }

    fn cancelled(done: Op) -> Self {
        Self {
            cancelled: true,
            ..Self::finished(done)
        }
    }

    /// The operation stopped before doing everything it planned.
    pub fn stopped_early(&self) -> bool {
        self.cancelled || self.error.is_some()
    }
}

pub fn run(
    op: &Op,
    trash: &dyn Trash,
    cancel: &Cancel,
    progress: &mut dyn FnMut(&Progress),
) -> Outcome {
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
) -> Outcome {
    match op {
        Op::Rename { from, to } => {
            if to.exists() && !same_item(from, to) {
                return Outcome::failed(Op::nothing(), OpsError::Exists(to.clone()));
            }
            match fs::rename(from, to) {
                Ok(()) => Outcome::finished(op.clone()),
                Err(e) => Outcome::failed(Op::nothing(), OpsError::io(from, e)),
            }
        }
        Op::NewFolder { path } => match fs::create_dir(path) {
            Ok(()) => Outcome::finished(op.clone()),
            Err(e) => Outcome::failed(Op::nothing(), OpsError::io(path, e)),
        },
        Op::Recycle { paths, .. } => {
            let recycled = trash.recycle(paths);
            Outcome {
                done: Op::Recycle {
                    paths: recycled.binned.iter().map(|b| b.original.clone()).collect(),
                    binned: recycled.binned,
                },
                cancelled: recycled.cancelled,
                error: recycled.error,
                deleted: recycled.deleted,
            }
        }
        Op::Restore { items, .. } => {
            let restored = trash.restore(items);
            let (items, restored_at) = restored.done.into_iter().unzip();
            let done = Op::Restore {
                items,
                restored: restored_at,
            };
            match restored.error {
                Some(err) => Outcome::failed(done, err),
                None => Outcome::finished(done),
            }
        }
        Op::Purge { items } => {
            let purged = trash.purge(items);
            let done = Op::Purge { items: purged.done };
            match purged.error {
                Some(err) => Outcome::failed(done, err),
                None => Outcome::finished(done),
            }
        }
        Op::Delete { paths } => {
            let mut done = Vec::new();
            for path in paths {
                report(progress, state, path);
                if cancel.is_cancelled() {
                    return Outcome::cancelled(Op::Delete { paths: done });
                }
                if let Err(err) = remove(path) {
                    return Outcome::failed(Op::Delete { paths: done }, err);
                }
                done.push(path.clone());
            }
            Outcome::finished(Op::Delete { paths: done })
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
            let mut deleted = Vec::new();
            for step in ops {
                let outcome = run_inner(step, trash, cancel, progress, state);
                deleted.extend(outcome.deleted);
                done.push(outcome.done);
                if outcome.cancelled || outcome.error.is_some() {
                    return Outcome {
                        done: Op::Batch { ops: done },
                        cancelled: outcome.cancelled,
                        error: outcome.error,
                        deleted,
                    };
                }
            }
            Outcome {
                deleted,
                ..Outcome::finished(Op::Batch { ops: done })
            }
        }
    }
}

/// How far one item of a move got.
enum Moved {
    Done,
    /// A move across volumes was cancelled after (part of) the copy, before the source was
    /// removed: the source is intact and the target is a copy to undo.
    CopyOnly,
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
) -> Outcome {
    let recycled = if replaced.is_empty() {
        Recycled::default()
    } else {
        trash.recycle(replaced)
    };
    let replaced_slots = recycled.binned;
    let rebuild = |pairs: Vec<Pair>| {
        let replaced = replaced_slots.iter().map(|b| b.original.clone()).collect();
        let replaced_slots = replaced_slots.clone();
        if moving {
            Op::Move {
                pairs,
                replaced,
                replaced_slots,
            }
        } else {
            Op::Copy {
                pairs,
                replaced,
                replaced_slots,
            }
        }
    };
    let mut out = Outcome {
        deleted: recycled.deleted,
        ..Outcome::finished(Op::nothing())
    };
    if recycled.error.is_some() || recycled.cancelled {
        // Something the user chose to replace is still there: copy nothing over it.
        out.done = rebuild(Vec::new());
        out.error = recycled.error;
        out.cancelled = recycled.cancelled;
        return out;
    }
    let mut done = Vec::new();
    let mut partial = None;
    for pair in pairs {
        report(progress, state, &pair.from);
        if cancel.is_cancelled() {
            out.cancelled = true;
            break;
        }
        if let Err(err) = check_target(pair) {
            out.error = Some(err);
            break;
        }
        let result = if moving {
            move_item(pair, cancel, state)
        } else {
            copy_tree(&pair.from, &pair.to, cancel, state).map(|()| Moved::Done)
        };
        match result {
            Ok(Moved::Done) => done.push(pair.clone()),
            Ok(Moved::CopyOnly) => {
                partial = Some(pair.clone());
                out.cancelled = true;
                break;
            }
            Err(err) => {
                // A copy that failed halfway left part of the item behind: undo recycles it. (A
                // failed move keeps its source, so its partial target is left for the user.)
                if !moving && fs::symlink_metadata(&pair.to).is_ok() {
                    done.push(pair.clone());
                }
                out.error = Some(err);
                break;
            }
        }
    }
    let main = rebuild(done);
    out.done = match partial {
        Some(pair) => Op::Batch {
            ops: vec![
                main,
                Op::Copy {
                    pairs: vec![pair],
                    replaced: Vec::new(),
                    replaced_slots: Vec::new(),
                },
            ],
        },
        None => main,
    };
    out
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
    if pair.from.is_dir() && crate::plan::within(&pair.to, &pair.from) {
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
fn move_item(pair: &Pair, cancel: &Cancel, state: &mut Progress) -> Result<Moved, OpsError> {
    if volume(&pair.from).is_some() && volume(&pair.from) == volume(&pair.to) {
        fs::rename(&pair.from, &pair.to).map_err(|e| OpsError::io(&pair.from, e))?;
        return Ok(Moved::Done);
    }
    copy_tree(&pair.from, &pair.to, cancel, state)?;
    if cancel.is_cancelled() {
        return Ok(Moved::CopyOnly);
    }
    let (source, copied) = (tree_size(&pair.from)?, tree_size(&pair.to)?);
    if source != copied {
        return Err(OpsError::VerifyFailed(pair.to.clone()));
    }
    remove(&pair.from)?;
    Ok(Moved::Done)
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
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::name;

    /// A Recycle Bin in a temp folder: recycled items are moved to numbered slots and back.
    pub struct FolderTrash {
        pub bin: PathBuf,
        next: AtomicUsize,
    }

    impl FolderTrash {
        pub fn new(bin: PathBuf) -> Self {
            fs::create_dir_all(&bin).unwrap();
            Self {
                bin,
                next: AtomicUsize::new(1),
            }
        }
    }

    impl Trash for FolderTrash {
        fn recycle(&self, paths: &[PathBuf]) -> Recycled {
            let mut out = Recycled::default();
            for path in paths {
                let slot = self
                    .bin
                    .join(format!("$R{}", self.next.fetch_add(1, Ordering::Relaxed)));
                if let Err(e) = fs::rename(path, &slot) {
                    out.error = Some(OpsError::io(path, e));
                    break;
                }
                out.binned.push(Binned {
                    original: path.clone(),
                    slot,
                });
            }
            out
        }

        fn purge(&self, items: &[Binned]) -> Purged {
            let mut out = Purged::default();
            for item in items {
                let gone = if item.slot.is_dir() {
                    fs::remove_dir_all(&item.slot)
                } else {
                    fs::remove_file(&item.slot)
                };
                match gone {
                    Ok(()) => out.done.push(item.clone()),
                    Err(e) => {
                        out.error.get_or_insert(OpsError::io(&item.slot, e));
                    }
                }
            }
            out
        }

        fn restore(&self, items: &[Binned]) -> Restored {
            let mut out = Restored::default();
            for item in items {
                if fs::symlink_metadata(&item.slot).is_err() {
                    out.error
                        .get_or_insert_with(|| OpsError::NotInRecycleBin(item.original.clone()));
                    continue;
                }
                let (Some(dir), Some(file)) = (item.original.parent(), item.original.file_name())
                else {
                    continue;
                };
                let target = dir.join(name::keep_both(dir, &file.to_string_lossy(), Path::exists));
                match fs::rename(&item.slot, &target) {
                    Ok(()) => out.done.push((item.clone(), target)),
                    Err(e) => {
                        out.error.get_or_insert(OpsError::io(&item.slot, e));
                    }
                }
            }
            out
        }
    }

    fn quiet() -> impl FnMut(&Progress) {
        |_| {}
    }

    fn undo(outcome: &Outcome, trash: &FolderTrash) -> Outcome {
        let outcome = run(
            &outcome.done.inverse().unwrap(),
            trash,
            &Cancel::default(),
            &mut quiet(),
        );
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        outcome
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
            replaced_slots: vec![],
        };
        let outcome = run(&copy, &trash, &cancel, &mut quiet());
        assert!(!outcome.stopped_early());
        assert_eq!(
            fs::read(dst.join("src").join("sub").join("b.txt")).unwrap(),
            b"world"
        );
        // Undo recycles the copy; the source is untouched.
        undo(&outcome, &trash);
        assert!(!dst.join("src").exists() && src.join("a.txt").exists());

        let rename = Op::Rename {
            from: src.join("a.txt"),
            to: src.join("A.txt"),
        };
        assert!(!run(&rename, &trash, &cancel, &mut quiet()).stopped_early());
        let moving = Op::Move {
            pairs: vec![Pair {
                from: src.join("A.txt"),
                to: dst.join("A.txt"),
            }],
            replaced: vec![],
            replaced_slots: vec![],
        };
        let outcome = run(&moving, &trash, &cancel, &mut quiet());
        assert!(dst.join("A.txt").exists());
        undo(&outcome, &trash);
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
            replaced_slots: vec![],
        };
        let outcome = run(&over, &trash, &cancel, &mut quiet());
        assert!(matches!(outcome.error, Some(OpsError::Exists(_))));
        assert!(outcome.done.is_noop());
        assert_eq!(fs::read(a.join("x.txt")).unwrap(), b"2");
        let into = Op::Copy {
            pairs: vec![Pair {
                from: a.clone(),
                to: a.join("inner"),
            }],
            replaced: vec![],
            replaced_slots: vec![],
        };
        let outcome = run(&into, &trash, &cancel, &mut quiet());
        assert!(matches!(outcome.error, Some(OpsError::IntoItself(_))));
    }

    #[test]
    fn replace_recycles_the_old_item_and_undo_brings_it_back() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        fs::write(root.path().join("x.txt"), b"new").unwrap();
        fs::write(dst.join("x.txt"), b"old").unwrap();
        let op = Op::Copy {
            pairs: vec![Pair {
                from: root.path().join("x.txt"),
                to: dst.join("x.txt"),
            }],
            replaced: vec![dst.join("x.txt")],
            replaced_slots: vec![],
        };
        let outcome = run(&op, &trash, &Cancel::default(), &mut quiet());
        assert_eq!(fs::read(dst.join("x.txt")).unwrap(), b"new");
        undo(&outcome, &trash);
        assert_eq!(fs::read(dst.join("x.txt")).unwrap(), b"old");
    }

    #[test]
    fn a_failure_keeps_what_was_done_for_undo() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let (a, gone, b) = (
            root.path().join("a.txt"),
            root.path().join("gone.txt"),
            root.path().join("b.txt"),
        );
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let op = Op::Recycle {
            paths: vec![a.clone(), gone, b.clone()],
            binned: vec![],
        };
        let outcome = run(&op, &trash, &Cancel::default(), &mut quiet());
        assert!(outcome.error.is_some());
        assert!(!a.exists() && b.exists(), "stops at the failing item");
        assert_eq!(outcome.done.item_count(), 1);
        undo(&outcome, &trash);
        assert_eq!(fs::read(&a).unwrap(), b"a");
    }

    #[test]
    fn restore_never_replaces_and_redo_recycles_what_came_back() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let file = root.path().join("x.txt");
        fs::write(&file, b"recycled").unwrap();
        let recycled = run(
            &Op::Recycle {
                paths: vec![file.clone()],
                binned: vec![],
            },
            &trash,
            &Cancel::default(),
            &mut quiet(),
        );
        // The user makes a new x.txt before undoing.
        fs::write(&file, b"new").unwrap();
        let restored = undo(&recycled, &trash);
        let back = root.path().join("x (2).txt");
        assert_eq!(fs::read(&back).unwrap(), b"recycled");
        assert_eq!(fs::read(&file).unwrap(), b"new");
        // Redo is the inverse of what the undo did: it recycles "x (2).txt", not the new file.
        undo(&restored, &trash);
        assert!(!back.exists());
        assert_eq!(fs::read(&file).unwrap(), b"new");
    }

    #[test]
    fn a_missing_slot_does_not_stop_the_rest_of_a_restore() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let (a, b) = (root.path().join("a.txt"), root.path().join("b.txt"));
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let recycled = run(
            &Op::Recycle {
                paths: vec![a.clone(), b.clone()],
                binned: vec![],
            },
            &trash,
            &Cancel::default(),
            &mut quiet(),
        );
        let Op::Recycle { binned, .. } = &recycled.done else {
            panic!("expected a recycle");
        };
        // The user empties the first item from the Recycle Bin.
        fs::remove_file(&binned[0].slot).unwrap();
        let outcome = run(
            &recycled.done.inverse().unwrap(),
            &trash,
            &Cancel::default(),
            &mut quiet(),
        );
        assert!(matches!(outcome.error, Some(OpsError::NotInRecycleBin(_))));
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"b");
        assert_eq!(outcome.done.item_count(), 1);
    }

    #[test]
    fn a_purge_keeps_what_it_deleted_and_has_no_undo() {
        let root = tempfile::tempdir().unwrap();
        let trash = FolderTrash::new(root.path().join("bin"));
        let (a, b) = (root.path().join("a.txt"), root.path().join("b.txt"));
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let recycled = run(
            &Op::Recycle {
                paths: vec![a.clone(), b.clone()],
                binned: vec![],
            },
            &trash,
            &Cancel::default(),
            &mut quiet(),
        );
        let Op::Recycle { binned, .. } = recycled.done else {
            panic!("expected a recycle");
        };
        // The second item was emptied elsewhere in the meantime.
        fs::remove_file(&binned[1].slot).unwrap();
        let outcome = run(
            &Op::Purge {
                items: binned.clone(),
            },
            &trash,
            &Cancel::default(),
            &mut quiet(),
        );
        assert!(outcome.error.is_some());
        assert!(!binned[0].slot.exists());
        assert_eq!(outcome.done.item_count(), 1);
        assert!(outcome.done.inverse().is_none(), "no undo for a purge");
        assert!(Op::Purge { items: vec![] }.is_noop());
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
            replaced_slots: vec![],
        };
        let outcome = run(&op, &trash, &cancel, &mut |p: &Progress| {
            seen += 1;
            if p.done == 2 {
                cancel.cancel();
            }
        });
        assert!(outcome.cancelled);
        assert_eq!(outcome.done.item_count(), 2, "{seen} reports");
    }
}
