//! The Recycle Bin through the shell (FEATURES.md F-OPS-02, F-OPS-07): recycling with
//! `IFileOperation` and restoring exactly those items for undo.
//!
//! A progress sink per item records what the shell did with it: the item's own slot in the bin
//! (`$R…`), a permanent delete after the shell's warning, or the error. Undo moves an item back
//! from that slot (renaming on collision, never replacing) and removes its `$I…` metadata file,
//! as Explorer's Restore does. Nothing is matched by name or time, so two recycles of the same
//! path never get mixed up.
//!
//! Both are blocking and need an STA on the calling thread (`StaGuard`); call them from an
//! operation worker, never the UI thread. The shell calls the sinks on that thread.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_NOERRORUI,
    FOF_RENAMEONCOLLISION, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_RECYCLEONDELETE, FileOperation,
    IFileOperation, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::PCWSTR;

use crate::error::ShellError;

/// What the shell did with a recycle request, item by item.
#[derive(Debug, Default)]
pub struct Recycled {
    /// `(original path as given, slot in the Recycle Bin)` of each item now in the bin.
    pub binned: Vec<(PathBuf, PathBuf)>,
    /// Items the bin could not take (too large, a network drive) that were deleted permanently
    /// after the user agreed in the shell's warning.
    pub deleted: Vec<PathBuf>,
    /// The user declined the shell's warning, so the remaining items stayed.
    pub cancelled: bool,
    /// Why the remaining items stayed (the first error).
    pub error: Option<ShellError>,
}

/// What a restore did, item by item.
#[derive(Debug, Default)]
pub struct Restored {
    /// `(index into the request, path it came back at)` of each restored item.
    pub done: Vec<(usize, PathBuf)>,
    /// Why the remaining items are still in the bin (the first error).
    pub error: Option<ShellError>,
}

fn shell_item(path: &Path) -> Result<IShellItem, ShellError> {
    let wide =
        sx_win::to_wide(path.as_os_str()).map_err(|e| ShellError::InvalidPath(e.to_string()))?;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no bind context.
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.map_err(|source| {
        ShellError::Open {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn file_system_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: the returned string is CoTaskMem-allocated; it is copied and freed here.
    unsafe {
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let out = PathBuf::from(OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.as_ptr().cast()));
        Some(out)
    }
}

fn file_operation(
    flags: FILEOPERATION_FLAGS,
    owner: Option<isize>,
) -> Result<IFileOperation, ShellError> {
    // SAFETY: creates the shell's file operation object in this thread's STA.
    let op: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(ShellError::Com)?;
    // SAFETY: plain setters on a live object; the owner handle is only used for dialogs.
    unsafe {
        op.SetOperationFlags(flags).map_err(ShellError::Com)?;
        if let Some(owner) = owner {
            op.SetOwnerWindow(HWND(owner as *mut core::ffi::c_void))
                .map_err(ShellError::Com)?;
        }
    }
    Ok(op)
}

/// The per-item progress sink. Its own module because the code `#[implement]` generates
/// trips two pedantic lints this crate cannot fix.
mod progress {
    #![allow(clippy::inline_always, clippy::ref_as_ptr)]

    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use windows::Win32::UI::Shell::{
        IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem, SICHINT_CANONICAL,
        SICHINT_TEST_FILESYSPATH_IF_NOT_EQUAL,
    };
    use windows::core::{HRESULT, PCWSTR, Ref};

    use super::file_system_path;

    /// What the shell reported for one queued item.
    #[derive(Debug)]
    pub(super) enum Report {
        /// Now at this path: its slot in the bin (recycle) or where it came back (restore).
        Placed(PathBuf),
        /// Done, but the shell gave no path for the result: deleted permanently (recycle).
        NoResult,
        Failed(HRESULT),
    }

    pub(super) type Slot = Rc<RefCell<Option<Report>>>;

    /// Records the shell's report about one item. The shell may also report items inside a folder
    /// (when it deletes one permanently); only the item itself counts.
    #[windows_core::implement(IFileOperationProgressSink)]
    struct Sink {
        item: IShellItem,
        report: Slot,
    }

    impl Sink {
        fn is_mine(&self, item: &Ref<IShellItem>) -> bool {
            let Some(item) = item.as_ref() else {
                return false;
            };
            let hint = (SICHINT_CANONICAL.0 | SICHINT_TEST_FILESYSPATH_IF_NOT_EQUAL.0) as u32;
            // SAFETY: compares two live shell items on this thread.
            let order = unsafe { self.item.Compare(item, hint) };
            order == Ok(0)
        }

        fn record(&self, item: &Ref<IShellItem>, result: HRESULT, created: &Ref<IShellItem>) {
            guarded(|| {
                if !self.is_mine(item) {
                    return;
                }
                let report = if result.is_err() {
                    Report::Failed(result)
                } else {
                    created
                        .as_ref()
                        .and_then(file_system_path)
                        .map_or(Report::NoResult, Report::Placed)
                };
                *self.report.borrow_mut() = Some(report);
            });
        }
    }

    /// Runs a sink callback body; a panic is logged and stops there, it never unwinds into the
    /// shell.
    fn guarded(body: impl FnOnce()) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err() {
            tracing::error!("panic in a file operation callback");
        }
    }

    // The callbacks other than PostDeleteItem/PostMoveItem return a constant and cannot panic.
    #[allow(non_snake_case)]
    impl IFileOperationProgressSink_Impl for Sink_Impl {
        fn StartOperations(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn FinishOperations(&self, _result: HRESULT) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreRenameItem(
            &self,
            _flags: u32,
            _item: Ref<IShellItem>,
            _name: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostRenameItem(
            &self,
            _flags: u32,
            _item: Ref<IShellItem>,
            _name: &PCWSTR,
            _result: HRESULT,
            _created: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreMoveItem(
            &self,
            _flags: u32,
            _item: Ref<IShellItem>,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostMoveItem(
            &self,
            _flags: u32,
            item: Ref<IShellItem>,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
            result: HRESULT,
            created: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            self.record(&item, result, &created);
            Ok(())
        }
        fn PreCopyItem(
            &self,
            _flags: u32,
            _item: Ref<IShellItem>,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostCopyItem(
            &self,
            _flags: u32,
            _item: Ref<IShellItem>,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
            _result: HRESULT,
            _created: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreDeleteItem(&self, _flags: u32, _item: Ref<IShellItem>) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostDeleteItem(
            &self,
            _flags: u32,
            item: Ref<IShellItem>,
            result: HRESULT,
            created: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            self.record(&item, result, &created);
            Ok(())
        }
        fn PreNewItem(
            &self,
            _flags: u32,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostNewItem(
            &self,
            _flags: u32,
            _folder: Ref<IShellItem>,
            _name: &PCWSTR,
            _template: &PCWSTR,
            _attributes: u32,
            _result: HRESULT,
            _created: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn UpdateProgress(&self, _total: u32, _so_far: u32) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
    }

    /// A sink for `item` and the slot it reports into.
    pub(super) fn sink(item: &IShellItem) -> (IFileOperationProgressSink, Slot) {
        let report = Slot::default();
        let sink = Sink {
            item: item.clone(),
            report: report.clone(),
        };
        (sink.into(), report)
    }
}

use progress::{Report, sink};

/// Runs the queued operations. `Ok(true)` when the user or the system stopped some of them.
fn perform(op: &IFileOperation) -> Result<bool, ShellError> {
    // SAFETY: runs the queued operations on this STA; may show shell dialogs owned by the owner
    // window, and calls the sinks on this thread.
    unsafe {
        let performed = op.PerformOperations();
        let aborted = op
            .GetAnyOperationsAborted()
            .is_ok_and(windows::core::BOOL::as_bool);
        match performed {
            Ok(()) => Ok(aborted),
            // Item failures are reported per item; this is the overall result.
            Err(_) if aborted => Ok(true),
            Err(err) => Err(ShellError::Com(err)),
        }
    }
}

/// Moves `paths` to the Recycle Bin and reports where each one went. If an item cannot be
/// recycled (too large, a network share), the shell asks before deleting it permanently
/// (`FOF_WANTNUKEWARNING`); nothing is destroyed silently.
pub fn recycle(paths: &[PathBuf], owner: Option<isize>) -> Recycled {
    let mut out = Recycled::default();
    let op = match file_operation(
        FOF_ALLOWUNDO
            | FOFX_RECYCLEONDELETE
            | FOF_NOCONFIRMATION
            | FOF_WANTNUKEWARNING
            | FOF_NOERRORUI
            | FOF_SILENT,
        owner,
    ) {
        Ok(op) => op,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    let mut queued = Vec::new();
    for path in paths {
        let queue = shell_item(path).and_then(|item| {
            let (sink, report) = sink(&item);
            // SAFETY: queues a delete of a live shell item with its own sink.
            unsafe { op.DeleteItem(&item, &sink) }.map_err(ShellError::Com)?;
            Ok(report)
        });
        match queue {
            Ok(report) => queued.push((path, report)),
            Err(err) => {
                out.error.get_or_insert(err);
            }
        }
    }
    if queued.is_empty() {
        return out;
    }
    let stopped = match perform(&op) {
        Ok(stopped) => stopped,
        Err(err) => {
            out.error.get_or_insert(err);
            false
        }
    };
    for (path, report) in queued {
        match report.borrow_mut().take() {
            Some(Report::Placed(slot)) => out.binned.push((path.clone(), slot)),
            Some(Report::NoResult) => out.deleted.push(path.clone()),
            Some(Report::Failed(code)) => {
                out.error.get_or_insert(ShellError::Item {
                    path: path.clone(),
                    source: code.into(),
                });
            }
            // Never reached: stopped by the user (the permanent-delete warning) or the system.
            None if stopped => out.cancelled = true,
            None => {
                out.error.get_or_insert(ShellError::Aborted);
            }
        }
    }
    out
}

/// The `$I…` file next to a `$R…` slot holds the item's original path and deletion time. The
/// bin lists an item from it, so it goes once the item is back (as with Explorer's Restore).
/// Only a `$I` file next to a `$R` slot inside a `$Recycle.Bin` folder is ever removed.
fn remove_metadata(slot: &Path) {
    let Some(name) = slot.file_name() else {
        return;
    };
    let mut wide: Vec<u16> = name.encode_wide().collect();
    let in_bin = slot.ancestors().any(|dir| {
        dir.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("$Recycle.Bin"))
    });
    if !in_bin || wide.len() < 3 || wide[0] != u16::from(b'$') || wide[1] != u16::from(b'R') {
        return;
    }
    wide[1] = u16::from(b'I');
    let info = slot.with_file_name(OsString::from_wide(&wide));
    if let Err(err) = std::fs::remove_file(&info) {
        tracing::debug!(error = %err, "recycle bin metadata not removed");
    }
}

/// Restores `(original path, slot)` items: each one is moved from its own slot back to its
/// original folder (recreated if it is gone). An item whose name is taken again comes back as
/// "name (2)" instead of replacing anything. A missing slot (the bin was emptied) does not stop
/// the others.
pub fn restore(items: &[(PathBuf, PathBuf)]) -> Restored {
    let mut out = Restored::default();
    let op = match file_operation(
        FOF_NOCONFIRMATION
            | FOF_NOCONFIRMMKDIR
            | FOF_RENAMEONCOLLISION
            | FOF_NOERRORUI
            | FOF_SILENT,
        None,
    ) {
        Ok(op) => op,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    let mut queued = Vec::new();
    for (ix, (original, slot)) in items.iter().enumerate() {
        let queue = (|| {
            if std::fs::symlink_metadata(slot).is_err() {
                return Err(ShellError::NotRecycled(original.clone()));
            }
            let (Some(parent), Some(name)) = (original.parent(), original.file_name()) else {
                return Err(ShellError::InvalidPath(original.display().to_string()));
            };
            std::fs::create_dir_all(parent).map_err(ShellError::Io)?;
            let (item, folder) = (shell_item(slot)?, shell_item(parent)?);
            let name = sx_win::to_wide(name).map_err(|e| ShellError::InvalidPath(e.to_string()))?;
            let (sink, report) = sink(&item);
            // SAFETY: queues a move of a live item into a live folder item with its own sink;
            // `name` outlives the call (the operation copies it).
            unsafe { op.MoveItem(&item, &folder, PCWSTR(name.as_ptr()), &sink) }
                .map_err(ShellError::Com)?;
            Ok(report)
        })();
        match queue {
            Ok(report) => queued.push((ix, report)),
            Err(err) => {
                out.error.get_or_insert(err);
            }
        }
    }
    if queued.is_empty() {
        return out;
    }
    if let Err(err) = perform(&op) {
        out.error.get_or_insert(err);
    }
    for (ix, report) in queued {
        let (original, slot) = &items[ix];
        match report.borrow_mut().take() {
            Some(Report::Placed(back)) => {
                remove_metadata(slot);
                out.done.push((ix, back));
            }
            Some(Report::Failed(code)) => {
                out.error.get_or_insert(ShellError::Item {
                    path: original.clone(),
                    source: code.into(),
                });
            }
            Some(Report::NoResult) | None => {
                out.error.get_or_insert(ShellError::Aborted);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StaGuard;

    #[test]
    fn metadata_is_only_removed_inside_a_bin() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("$Recycle.Bin").join("S-1-5-21-1");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["$IABC.txt", "$RABC.txt"] {
            std::fs::write(bin.join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("$IABC.txt"), b"x").unwrap();
        // Outside a bin, or not a `$R` slot: nothing is touched.
        remove_metadata(&dir.path().join("$RABC.txt"));
        remove_metadata(&bin.join("ABC.txt"));
        assert!(dir.path().join("$IABC.txt").exists() && bin.join("$IABC.txt").exists());
        remove_metadata(&bin.join("$RABC.txt"));
        assert!(!bin.join("$IABC.txt").exists() && bin.join("$RABC.txt").exists());
    }

    /// Uses the real Recycle Bin of this user (it leaves nothing behind when it passes):
    /// `cargo test -p sx-shell real_recycle_bin -- --ignored`.
    #[test]
    #[ignore = "uses the real Recycle Bin"]
    fn real_recycle_bin_round_trip() {
        let _com = StaGuard::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join(format!("sx-recycle-test-{}.txt", std::process::id()));
        std::fs::write(&file, b"keep me").unwrap();
        let recycled = recycle(std::slice::from_ref(&file), None);
        assert!(recycled.error.is_none(), "{:?}", recycled.error);
        let [(original, slot)] = recycled.binned.as_slice() else {
            panic!("expected one binned item: {recycled:?}");
        };
        assert_eq!(original, &file);
        assert!(!file.exists() && slot.exists(), "{}", slot.display());
        let info = remove_metadata_path(slot);
        assert!(info.exists(), "{}", info.display());

        // Someone makes a new file with the same name; restore must not replace it.
        std::fs::write(&file, b"new").unwrap();
        let restored = restore(&recycled.binned);
        assert!(restored.error.is_none(), "{:?}", restored.error);
        let [(0, back)] = restored.done.as_slice() else {
            panic!("expected one restored item: {restored:?}");
        };
        assert_ne!(back, &file);
        assert_eq!(std::fs::read(back).unwrap(), b"keep me");
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert!(!slot.exists() && !info.exists(), "no ghost left in the bin");
    }

    fn remove_metadata_path(slot: &Path) -> PathBuf {
        let name = slot
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replacen("$R", "$I", 1);
        slot.with_file_name(name)
    }
}
