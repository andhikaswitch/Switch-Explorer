//! The Recycle Bin through the shell (FEATURES.md F-OPS-02, F-OPS-07): recycling with
//! `IFileOperation` and restoring recycled items for undo.
//!
//! Both are blocking and need an STA on the calling thread (`StaGuard`); call them from an
//! operation worker, never the UI thread.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{FILETIME, HWND, PROPERTYKEY};
use windows::Win32::Storage::EnhancedStorage::PKEY_FileName;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR,
    FOF_NOERRORUI, FOF_RENAMEONCOLLISION, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_RECYCLEONDELETE,
    FOLDERID_RecycleBinFolder, FileOperation, IEnumShellItems, IFileOperation, IShellItem,
    IShellItem2, KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderItem,
};
use windows::core::{GUID, Interface as _, PCWSTR, PWSTR};

use crate::error::ShellError;

/// `System.Recycle.DeletedFrom` (the folder an item was deleted from).
const PKEY_RECYCLE_DELETED_FROM: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9b17_4b33_40ff_11d2_a27e_00c0_4fc3_0871),
    pid: 2,
};
/// `System.Recycle.DateDeleted`.
const PKEY_RECYCLE_DATE_DELETED: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9b17_4b33_40ff_11d2_a27e_00c0_4fc3_0871),
    pid: 3,
};

/// Seconds since the epoch, one second early so that an item recycled right after counts as
/// "at or after" this stamp despite clock granularity.
fn stamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().saturating_sub(1))
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

fn perform(op: &IFileOperation) -> Result<(), ShellError> {
    // SAFETY: runs the queued operations; may show shell dialogs owned by the owner window.
    unsafe {
        op.PerformOperations().map_err(ShellError::Com)?;
        if op
            .GetAnyOperationsAborted()
            .map_err(ShellError::Com)?
            .as_bool()
        {
            return Err(ShellError::Aborted);
        }
    }
    Ok(())
}

/// Moves `paths` to the Recycle Bin and returns a stamp for [`restore`]. If an item cannot be
/// recycled (too large, a network share), the shell asks before deleting it permanently
/// (`FOF_WANTNUKEWARNING`); nothing is destroyed silently.
pub fn recycle(paths: &[PathBuf], owner: Option<isize>) -> Result<u64, ShellError> {
    let stamp = stamp_now();
    let op = file_operation(
        FOF_ALLOWUNDO
            | FOFX_RECYCLEONDELETE
            | FOF_NOCONFIRMATION
            | FOF_WANTNUKEWARNING
            | FOF_NOERRORUI
            | FOF_SILENT,
        owner,
    )?;
    for path in paths {
        let item = shell_item(path)?;
        // SAFETY: queues a delete of a live shell item.
        unsafe { op.DeleteItem(&item, None) }.map_err(ShellError::Com)?;
    }
    perform(&op)?;
    Ok(stamp)
}

/// One recycled item.
struct Recycled {
    item: IShellItem,
    original: PathBuf,
    name: OsString,
    deleted: u64,
}

fn string_property(item: &IShellItem2, key: &PROPERTYKEY) -> Option<OsString> {
    // SAFETY: the returned string is CoTaskMem-allocated; it is copied and freed here.
    unsafe {
        let value: PWSTR = item.GetString(key).ok()?;
        let out = OsString::from_wide(value.as_wide());
        CoTaskMemFree(Some(value.as_ptr().cast()));
        Some(out)
    }
}

fn filetime_secs(ft: FILETIME) -> u64 {
    // FILETIME counts 100 ns since 1601; the Unix epoch is 11 644 473 600 s later.
    let ticks = (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
    (ticks / 10_000_000).saturating_sub(11_644_473_600)
}

fn recycled_items() -> Result<Vec<Recycled>, ShellError> {
    // SAFETY: shell item and enumeration objects are used on this STA thread only.
    unsafe {
        let bin: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)
                .map_err(ShellError::Com)?;
        let items: IEnumShellItems = bin
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(ShellError::Com)?;
        let mut out = Vec::new();
        loop {
            let mut slot = [None];
            let mut fetched = 0u32;
            if items.Next(&mut slot, Some(&raw mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let Ok(item2) = item.cast::<IShellItem2>() else {
                continue;
            };
            let (Some(from), Some(name)) = (
                string_property(&item2, &PKEY_RECYCLE_DELETED_FROM),
                string_property(&item2, &PKEY_FileName),
            ) else {
                continue;
            };
            let deleted = item2
                .GetFileTime(&PKEY_RECYCLE_DATE_DELETED)
                .map_or(0, filetime_secs);
            out.push(Recycled {
                original: PathBuf::from(&from).join(&name),
                name,
                item,
                deleted,
            });
        }
        Ok(out)
    }
}

/// Restores recycled items to their original paths: for each path, the earliest item recycled
/// at or after `since`, or the most recent one without it. An item whose name is taken again
/// comes back as "name (2)" instead of replacing anything.
pub fn restore(paths: &[PathBuf], since: Option<u64>) -> Result<(), ShellError> {
    let mut recycled = recycled_items()?;
    let op = file_operation(
        FOF_NOCONFIRMATION
            | FOF_NOCONFIRMMKDIR
            | FOF_RENAMEONCOLLISION
            | FOF_NOERRORUI
            | FOF_SILENT,
        None,
    )?;
    let mut queued = 0;
    for path in paths {
        let same = |r: &Recycled| {
            r.original
                .as_os_str()
                .eq_ignore_ascii_case(path.as_os_str())
        };
        let pick = match since {
            Some(since) => recycled
                .iter()
                .enumerate()
                .filter(|(_, r)| same(r) && r.deleted >= since)
                .min_by_key(|(_, r)| r.deleted),
            None => recycled
                .iter()
                .enumerate()
                .filter(|(_, r)| same(r))
                .max_by_key(|(_, r)| r.deleted),
        }
        .map(|(ix, _)| ix);
        let Some(ix) = pick else {
            return Err(ShellError::NotRecycled(path.clone()));
        };
        let entry = recycled.swap_remove(ix);
        let Some(parent) = entry.original.parent() else {
            continue;
        };
        std::fs::create_dir_all(parent).map_err(ShellError::Io)?;
        let folder = shell_item(parent)?;
        let name =
            sx_win::to_wide(&entry.name).map_err(|e| ShellError::InvalidPath(e.to_string()))?;
        // SAFETY: queues a move of a recycled item into a live folder item; `name` outlives it.
        unsafe { op.MoveItem(&entry.item, &folder, PCWSTR(name.as_ptr()), None) }
            .map_err(ShellError::Com)?;
        queued += 1;
    }
    if queued > 0 {
        perform(&op)?;
    }
    Ok(())
}

/// How long ago the Recycle Bin was last asked, for tests that wait for the shell.
#[doc(hidden)]
pub const SHELL_SETTLE: Duration = Duration::from_millis(200);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StaGuard;

    #[test]
    fn recycle_and_restore_a_file() {
        let _com = StaGuard::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join(format!("sx-recycle-test-{}.txt", std::process::id()));
        std::fs::write(&file, b"keep me").unwrap();
        let stamp = recycle(std::slice::from_ref(&file), None).unwrap();
        assert!(!file.exists());
        restore(std::slice::from_ref(&file), Some(stamp)).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"keep me");
    }
}
