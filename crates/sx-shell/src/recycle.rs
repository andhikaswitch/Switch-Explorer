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
    BHID_EnumItems, FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR,
    FOF_NOERRORUI, FOF_RENAMEONCOLLISION, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_RECYCLEONDELETE,
    FOLDERID_RecycleBinFolder, FileOperation, IEnumShellItems, IFileOperation, IShellItem,
    KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderItem, SIGDN_FILESYSPATH,
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

/// An item in the Recycle Bin, for its page (FEATURES.md F-LOC-02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinItem {
    /// Its slot in the bin (`…\$Recycle.Bin\<SID>\$R…`).
    pub slot: PathBuf,
    /// Where it was deleted from, with its name.
    pub original: PathBuf,
    /// Its size when deleted (a folder: everything in it).
    pub size: u64,
    /// When it was deleted (FILETIME ticks; 0 when unknown).
    pub deleted: i64,
    pub folder: bool,
}

/// What a `$I…` file says about its item: original path, size, deletion time.
#[derive(Debug, PartialEq, Eq)]
struct BinRecord {
    original: PathBuf,
    size: u64,
    /// FILETIME ticks; 0 when unknown.
    deleted: i64,
}

/// Reads a `$I…` record. Version 1 (Vista to 8.1) has a fixed 260-character path, version 2
/// (Windows 10+) a length and then the path. Anything else is not a record.
fn parse_record(bytes: &[u8]) -> Option<BinRecord> {
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
    };
    let version = u64_at(0)?;
    let size = u64_at(8)?;
    let filetime = u64_at(16)?;
    let units: Vec<u16> = match version {
        1 => bytes
            .get(24..24 + 520)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect(),
        2 => {
            let len = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?) as usize;
            bytes
                .get(28..28usize.checked_add(len.checked_mul(2)?)?)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect()
        }
        _ => return None,
    };
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    if end == 0 {
        return None;
    }
    let deleted = i64::try_from(filetime).unwrap_or(0);
    Some(BinRecord {
        original: PathBuf::from(OsString::from_wide(&units[..end])),
        size,
        deleted,
    })
}

/// The largest `$I…` record: a version 2 header and a path of 32 767 characters and its NUL.
const MAX_RECORD: u64 = 28 + 2 * 32_768;

/// A `$I…` file's bytes, never more than a record can hold: a giant file planted in a bin on a
/// drive without permissions (FAT, exFAT) is refused, not read into memory.
fn read_record(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_RECORD {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Whether a record's original path can be where its item came from: absolute, without `.` or
/// `..`, on the slot's own drive (or share). A restore never goes anywhere else.
fn plausible(original: &Path, slot: &Path) -> bool {
    use std::path::{Component, Prefix};
    fn volume(path: &Path) -> Option<String> {
        match path.components().next()? {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    Some(format!("{}:", char::from(letter).to_ascii_lowercase()))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
                    r"\\{}\{}",
                    server.to_string_lossy().to_lowercase(),
                    share.to_string_lossy().to_lowercase()
                )),
                _ => None,
            },
            _ => None,
        }
    }
    let mut parts = original.components();
    let rooted = matches!(parts.next(), Some(Component::Prefix(_)))
        && matches!(parts.next(), Some(Component::RootDir));
    let clean = original
        .components()
        .all(|part| !matches!(part, Component::CurDir | Component::ParentDir));
    rooted && clean && volume(original).is_some() && volume(original) == volume(slot)
}

/// The `$I…` record next to a `$R…` slot.
fn record_of(slot: &Path) -> Option<BinRecord> {
    let name: Vec<u16> = slot.file_name()?.encode_wide().collect();
    if name.len() < 3 || name[0] != u16::from(b'$') || name[1] != u16::from(b'R') {
        return None;
    }
    let mut info = name;
    info[1] = u16::from(b'I');
    parse_record(&read_record(
        &slot.with_file_name(OsString::from_wide(&info)),
    )?)
}

/// What the bin page lists.
#[derive(Debug, Default)]
pub struct BinListing {
    pub items: Vec<BinItem>,
    /// Items left out: no readable record, or one whose origin cannot be where they came from.
    pub skipped: usize,
}

/// What is in the Recycle Bin, on every drive (the shell's own view of it). Blocking; needs an
/// STA (it enters one): call it off the UI thread. An item without a readable, plausible record
/// is left out (and counted) rather than shown without its origin.
pub fn list_bin() -> Result<BinListing, ShellError> {
    let _com = crate::com::StaGuard::new()?;
    let mut out = BinListing::default();
    // SAFETY: COM calls on this thread's STA; every item is used and released here.
    unsafe {
        let bin: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)
                .map_err(ShellError::Com)?;
        let items: IEnumShellItems = bin
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(ShellError::Com)?;
        loop {
            let mut got = [None];
            let mut fetched = 0u32;
            if let Err(err) = items.Next(&mut got, Some(&raw mut fetched)) {
                // Nothing listed at all: say so rather than show an empty bin.
                if out.items.is_empty() {
                    return Err(ShellError::Com(err));
                }
                tracing::debug!(error = ?err, "recycle bin listing stopped early");
                break;
            }
            if fetched == 0 {
                break;
            }
            let Some(item) = got[0].take() else {
                break;
            };
            let Some(slot) = file_system_path(&item) else {
                out.skipped += 1;
                continue;
            };
            let Some(record) = record_of(&slot).filter(|r| plausible(&r.original, &slot)) else {
                out.skipped += 1;
                continue;
            };
            out.items.push(BinItem {
                folder: slot.is_dir(),
                slot,
                original: record.original,
                size: record.size,
                deleted: record.deleted,
            });
        }
    }
    Ok(out)
}

/// A requested item's index and the bin's own shell item for it.
type Listed = (usize, IShellItem);

/// The bin's own shell items for the `(original path, slot)` items still in their slots and
/// still recording that original, with their index in `items`; and the error that ended the
/// enumeration early, if one did.
fn listed_items(
    items: &[(PathBuf, PathBuf)],
) -> Result<(Vec<Listed>, Option<windows::core::Error>), ShellError> {
    let key = |path: &Path| path.as_os_str().to_string_lossy().to_lowercase();
    let wanted: std::collections::HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(ix, (_, slot))| (key(slot), ix))
        .collect();
    let mut stopped_early = None;
    // SAFETY: COM calls on this thread's STA; the items stay on this thread (the caller's) and
    // are released when it drops them.
    let found = unsafe {
        let bin: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)
                .map_err(ShellError::Com)?;
        let enumerator: IEnumShellItems = bin
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(ShellError::Com)?;
        let mut found = Vec::new();
        loop {
            let mut got = [None];
            let mut fetched = 0u32;
            if let Err(err) = enumerator.Next(&mut got, Some(&raw mut fetched)) {
                stopped_early = Some(err);
                break;
            }
            if fetched == 0 {
                break;
            }
            let Some(item) = got[0].take() else {
                break;
            };
            let Some(slot) = file_system_path(&item) else {
                continue;
            };
            let Some(&ix) = wanted.get(&key(&slot)) else {
                continue;
            };
            // Still the item that was listed.
            if record_of(&slot).is_some_and(|r| same_path(&r.original, &items[ix].0)) {
                found.push((ix, item));
            }
        }
        found
    };
    Ok((found, stopped_early))
}

/// What a purge did, item by item.
#[derive(Debug, Default)]
pub struct Purged {
    /// Indexes into the request of the items deleted for good.
    pub done: Vec<usize>,
    /// Why the remaining items are still in the bin (the first error).
    pub error: Option<ShellError>,
}

/// Deletes `(original path, slot)` items from the Recycle Bin for good, as Explorer does: the
/// shell deletes its own bin items, so their records go too, read-only items go, and every view
/// of the bin updates. Only items still in their slot whose record still names the original are
/// deleted (a slot emptied elsewhere and reused is left alone). No undo, no confirmation (the
/// app asked), no error dialogs. Blocking; needs an STA on the calling thread (`StaGuard`).
pub fn purge(items: &[(PathBuf, PathBuf)], owner: Option<isize>) -> Purged {
    let mut out = Purged::default();
    let (found, stopped_early) = match listed_items(items) {
        Ok(listed) => listed,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    if found.len() < items.len() {
        let seen: std::collections::HashSet<usize> = found.iter().map(|(ix, _)| *ix).collect();
        let missing = (0..items.len()).find(|ix| !seen.contains(ix));
        out.error = match (stopped_early, missing) {
            (Some(err), Some(_)) => Some(ShellError::Com(err)),
            (None, Some(ix)) => Some(ShellError::NotRecycled(items[ix].0.clone())),
            _ => None,
        };
    }
    if found.is_empty() {
        return out;
    }
    let op = match file_operation(FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT, owner) {
        Ok(op) => op,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    let mut queued = Vec::with_capacity(found.len());
    for (ix, item) in &found {
        let (sink, report) = sink(item);
        // SAFETY: the item, its sink and the operation live on this STA for the whole call.
        match unsafe { op.DeleteItem(item, &sink) } {
            Ok(()) => queued.push((*ix, report)),
            Err(err) => {
                out.error.get_or_insert(ShellError::Item {
                    path: items[*ix].0.clone(),
                    source: err,
                });
            }
        }
    }
    let stopped = match perform(&op) {
        Ok(stopped) => stopped,
        Err(err) => {
            out.error.get_or_insert(err);
            false
        }
    };
    for (ix, report) in queued {
        let (original, slot) = &items[ix];
        // Done only when the slot is really gone (not merely unreadable).
        let gone = std::fs::symlink_metadata(slot)
            .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound);
        if gone {
            out.done.push(ix);
            continue;
        }
        let report = report.borrow_mut().take();
        let error = match report {
            Some(Report::Failed(code)) => ShellError::Item {
                path: original.clone(),
                source: code.into(),
            },
            _ if stopped => ShellError::Aborted,
            // The shell said nothing failed, yet the item is still there.
            _ => ShellError::NotRecycled(original.clone()),
        };
        out.error.get_or_insert(error);
    }
    out
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

    use windows::Win32::Foundation::S_OK;
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
        /// `S_OK` and no resulting item: deleted permanently, if the item is really gone
        /// (recycle).
        Gone,
        /// Another success code without a result (`COPYENGINE_S_USER_IGNORED` when the user
        /// declined the permanent-delete warning): left where it was.
        Skipped,
        /// A resulting item the shell gave no file system path for.
        Unplaced,
        Failed(HRESULT),
    }

    pub(super) type Slot = Rc<RefCell<Option<Report>>>;

    /// Records the shell's report about one item. The shell may also report items inside a folder
    /// (when it deletes one permanently); only the item itself counts.
    ///
    /// Not agile: its state is single-threaded (`Rc`, an STA shell item), so COM calls it on the
    /// STA that made it and marshals calls from any other apartment.
    #[windows_core::implement(IFileOperationProgressSink, Agile = false)]
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
                } else if let Some(created) = created.as_ref() {
                    file_system_path(created).map_or(Report::Unplaced, Report::Placed)
                } else if result == S_OK {
                    Report::Gone
                } else {
                    Report::Skipped
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
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        // Each item once: a second delete of the same item would fail and hide the first.
        if !seen.insert(path) {
            continue;
        }
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
        let report = report.borrow_mut().take();
        match report {
            Some(Report::Placed(slot)) => out.binned.push((path.clone(), slot)),
            // Only what is really gone was deleted; anything still there was left alone.
            Some(Report::Gone) if std::fs::symlink_metadata(path).is_err() => {
                out.deleted.push(path.clone());
            }
            Some(Report::Gone | Report::Skipped) => out.cancelled = true,
            Some(Report::Unplaced) => {
                // In the bin, but without a slot undo could find it by.
                out.error
                    .get_or_insert(ShellError::NotRecycled(path.clone()));
            }
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

/// The original path the `$I…` record of a `$R…` slot names (read with the same bounds as a
/// listing). `None` when it cannot be read.
fn recorded_original(slot: &Path) -> Option<PathBuf> {
    record_of(slot).map(|record| record.original)
}

/// Whether two paths name the same item, ignoring a verbatim prefix and ASCII case.
fn same_path(a: &Path, b: &Path) -> bool {
    let plain = |p: &Path| {
        let s = p.as_os_str().to_string_lossy().into_owned();
        s.strip_prefix(r"\\?\")
            .map_or_else(|| s.clone(), str::to_owned)
    };
    plain(a).eq_ignore_ascii_case(&plain(b))
}

/// Where an item comes back: its original path, or the first free "name (2).ext" when that is
/// taken again (as Explorer numbers). Chosen before the move, so the shell never meets a
/// collision, where it could merge a folder into the new one instead of renaming.
fn free_target(original: &Path, folder: bool) -> Option<PathBuf> {
    if std::fs::symlink_metadata(original).is_err() {
        return Some(original.to_path_buf());
    }
    let parent = original.parent()?;
    let (stem, ext) = match (folder, original.extension()) {
        (false, Some(ext)) => (original.file_stem()?, Some(ext)),
        _ => (original.file_name()?, None),
    };
    (2..10_000).find_map(|n| {
        let mut name = stem.to_os_string();
        name.push(format!(" ({n})"));
        if let Some(ext) = ext {
            name.push(".");
            name.push(ext);
        }
        let candidate = parent.join(name);
        std::fs::symlink_metadata(&candidate)
            .is_err()
            .then_some(candidate)
    })
}

/// Whether `slot` is a `$R…` slot in a user's folder (`S-1-…`) of the `$Recycle.Bin` folder at
/// the root of a volume.
fn in_bin(slot: &Path) -> bool {
    let is_slot = slot.file_name().is_some_and(|n| {
        let wide: Vec<u16> = n.encode_wide().collect();
        wide.len() >= 3 && wide[0] == u16::from(b'$') && wide[1] == u16::from(b'R')
    });
    // The slot's folder is the user's (`S-1-…`), and that folder sits in `$Recycle.Bin`.
    let sid = slot.parent();
    is_slot
        && sid
        .and_then(Path::file_name)
        .is_some_and(|n| n.to_string_lossy().starts_with("S-1-"))
        && sid
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_some_and(|n| n.eq_ignore_ascii_case("$Recycle.Bin"))
        // ... at the root of its volume.
        && sid
            .and_then(Path::parent)
            .and_then(Path::parent)
            .is_some_and(|root| root.file_name().is_none())
}

/// The `$I…` file next to a `$R…` slot holds the item's original path and deletion time. The
/// bin lists an item from it, so it goes once the item is back (as with Explorer's Restore).
/// Only a `$I` file next to a `$R` slot inside a `$Recycle.Bin` folder is ever removed.
fn remove_metadata(slot: &Path) {
    let Some(name) = slot.file_name() else {
        return;
    };
    if !in_bin(slot) {
        return;
    }
    let mut wide: Vec<u16> = name.encode_wide().collect();
    wide[1] = u16::from(b'I');
    let info = slot.with_file_name(OsString::from_wide(&wide));
    if let Err(err) = std::fs::remove_file(&info) {
        tracing::debug!(error = %err, "recycle bin metadata not removed");
    }
}

/// Restores `(original path, slot)` items: each one is moved from its own slot back to its
/// original folder (recreated if it is gone). An item whose name is taken again comes back as
/// "name (2)" instead of replacing anything. A missing slot (the bin was emptied), or one whose
/// metadata names another item, does not stop the others.
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
            let Ok(meta) = std::fs::symlink_metadata(slot) else {
                return Err(ShellError::NotRecycled(original.clone()));
            };
            // The bin reuses slot names once emptied: never bring back someone else's item.
            if recorded_original(slot).is_some_and(|recorded| !same_path(&recorded, original)) {
                return Err(ShellError::NotRecycled(original.clone()));
            }
            let invalid = || ShellError::InvalidPath(original.display().to_string());
            let target = free_target(original, meta.is_dir()).ok_or_else(invalid)?;
            let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
                return Err(invalid());
            };
            std::fs::create_dir_all(parent).map_err(ShellError::Io)?;
            let (item, folder) = (shell_item(slot)?, shell_item(parent)?);
            let name = sx_win::to_wide(name).map_err(|e| ShellError::InvalidPath(e.to_string()))?;
            let (sink, report) = sink(&item);
            // SAFETY: queues a move of a live item into a live folder item with its own sink;
            // `name` outlives the call (the operation copies it).
            unsafe { op.MoveItem(&item, &folder, PCWSTR(name.as_ptr()), &sink) }
                .map_err(ShellError::Com)?;
            Ok((report, target))
        })();
        match queue {
            Ok((report, target)) => queued.push((ix, report, target)),
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
    for (ix, report, target) in queued {
        let (original, slot) = &items[ix];
        let report = report.borrow_mut().take();
        match report {
            Some(Report::Placed(back)) => {
                remove_metadata(slot);
                out.done.push((ix, back));
            }
            // Moved, but without a path from the shell: it is where it was asked to go.
            Some(Report::Unplaced) => {
                remove_metadata(slot);
                out.done.push((ix, target));
            }
            Some(Report::Failed(code)) => {
                out.error.get_or_insert(ShellError::Item {
                    path: original.clone(),
                    source: code.into(),
                });
            }
            Some(Report::Gone | Report::Skipped) | None => {
                out.error.get_or_insert(ShellError::Aborted);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn origins_must_be_where_the_slot_is() {
        let slot = Path::new(r"C:\$Recycle.Bin\S-1-5-21\$RAB12.txt");
        assert!(super::plausible(Path::new(r"C:\Data\notes.txt"), slot));
        assert!(!super::plausible(Path::new(r"D:\Data\notes.txt"), slot));
        assert!(!super::plausible(Path::new(r"Data\notes.txt"), slot));
        assert!(!super::plausible(Path::new(r"C:\Data\..\Windows\x"), slot));
        assert!(!super::plausible(Path::new(r"\\host\share\x"), slot));
        let share = Path::new(r"\\host\share\$Recycle.Bin\S-1-5-21\$RAB12.txt");
        assert!(super::plausible(Path::new(r"\\HOST\Share\docs\x"), share));
    }

    #[test]
    fn a_giant_record_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("$Ibig");
        std::fs::write(
            &big,
            vec![0u8; usize::try_from(super::MAX_RECORD).unwrap() + 1],
        )
        .unwrap();
        assert_eq!(super::read_record(&big), None);
        let fine = dir.path().join("$Ifine");
        std::fs::write(&fine, [1u8; 40]).unwrap();
        assert_eq!(super::read_record(&fine).map(|b| b.len()), Some(40));
    }

    #[test]
    fn hostile_record_lengths_are_refused() {
        let mut v2 = Vec::new();
        v2.extend_from_slice(&2u64.to_le_bytes());
        v2.extend_from_slice(&0u64.to_le_bytes());
        v2.extend_from_slice(&0u64.to_le_bytes());
        v2.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(super::parse_record(&v2), None, "length beyond the data");
        let mut empty = v2.clone();
        empty[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(super::parse_record(&empty), None, "no path");
        assert_eq!(super::parse_record(&v2[..20]), None, "truncated header");
    }

    #[test]
    fn bin_records_of_both_versions_are_read() {
        let path: Vec<u16> = r"C:\Data\notes.txt".encode_utf16().collect();
        // Version 2: length (with the NUL), then the path.
        let mut v2 = Vec::new();
        v2.extend_from_slice(&2u64.to_le_bytes());
        v2.extend_from_slice(&1234u64.to_le_bytes());
        v2.extend_from_slice(&133_000_000_000_000_000u64.to_le_bytes());
        v2.extend_from_slice(&(path.len() as u32 + 1).to_le_bytes());
        for unit in path.iter().chain(std::iter::once(&0)) {
            v2.extend_from_slice(&unit.to_le_bytes());
        }
        let record = super::parse_record(&v2).unwrap();
        assert_eq!(record.original, PathBuf::from(r"C:\Data\notes.txt"));
        assert_eq!(record.size, 1234);
        assert_eq!(record.deleted, 133_000_000_000_000_000);
        // Version 1: a fixed 260-character field.
        let mut v1 = Vec::new();
        v1.extend_from_slice(&1u64.to_le_bytes());
        v1.extend_from_slice(&7u64.to_le_bytes());
        v1.extend_from_slice(&0u64.to_le_bytes());
        let mut field = [0u16; 260];
        field[..path.len()].copy_from_slice(&path);
        for unit in field {
            v1.extend_from_slice(&unit.to_le_bytes());
        }
        let record = super::parse_record(&v1).unwrap();
        assert_eq!(record.original, PathBuf::from(r"C:\Data\notes.txt"));
        assert_eq!(record.deleted, 0, "a zero time is no time");
        // Not a record: unknown version, truncated, empty path.
        assert_eq!(super::parse_record(&[0u8; 10]), None);
        let mut v3 = v2.clone();
        v3[0] = 3;
        assert_eq!(super::parse_record(&v3), None);
        assert_eq!(super::parse_record(&v2[..30]), None);
    }

    use super::*;
    use crate::StaGuard;

    #[test]
    fn metadata_is_only_removed_inside_a_bin() {
        assert!(in_bin(Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\$RABC.txt")));
        assert!(in_bin(Path::new(r"\\?\D:\$Recycle.Bin\S-1-5-21-1\$RABC")));
        // Not at the root of a volume, not a user's folder, or not a `$R` slot.
        assert!(!in_bin(Path::new(
            r"C:\Data\$Recycle.Bin\S-1-5-21-1\$RABC.txt"
        )));
        assert!(!in_bin(Path::new(r"C:\$Recycle.Bin\Other\$RABC.txt")));
        assert!(!in_bin(Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\ABC.txt")));
        assert!(!in_bin(Path::new(r"C:\$RABC.txt")));
        // A bin-like folder elsewhere is never touched.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("$Recycle.Bin").join("S-1-5-21-1");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("$IABC.txt"), b"x").unwrap();
        remove_metadata(&bin.join("$RABC.txt"));
        assert!(bin.join("$IABC.txt").exists());
    }

    #[test]
    fn free_targets_number_like_explorer() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("report.txt");
        assert_eq!(free_target(&file, false), Some(file.clone()));
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(dir.path().join("report (2).txt"), b"x").unwrap();
        assert_eq!(
            free_target(&file, false),
            Some(dir.path().join("report (3).txt"))
        );
        // Folders keep dots in their name.
        let folder = dir.path().join("v1.2");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            free_target(&folder, true),
            Some(dir.path().join("v1.2 (2)"))
        );
    }

    #[test]
    fn metadata_names_the_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let slot = dir.path().join("$RABC123.txt");
        let original = r"C:\Users\Public\a.txt";
        let wide: Vec<u16> = original.encode_utf16().chain(std::iter::once(0)).collect();
        let mut bytes = Vec::new();
        bytes.extend(2u64.to_le_bytes());
        bytes.extend(5u64.to_le_bytes());
        bytes.extend(0u64.to_le_bytes());
        bytes.extend((wide.len() as u32).to_le_bytes());
        bytes.extend(wide.iter().flat_map(|u| u.to_le_bytes()));
        std::fs::write(dir.path().join("$IABC123.txt"), bytes).unwrap();
        let recorded = recorded_original(&slot).unwrap();
        assert_eq!(recorded, PathBuf::from(original));
        assert!(same_path(
            &recorded,
            Path::new(r"\\?\c:\users\public\A.TXT")
        ));
        assert!(!same_path(&recorded, Path::new(r"C:\Users\Public\b.txt")));
        assert_eq!(recorded_original(&dir.path().join("plain.txt")), None);
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

        // A folder whose name is taken again comes back beside the new one, never merged.
        let folder = dir.path().join("sx-recycle-folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("old.txt"), b"old").unwrap();
        let recycled = recycle(std::slice::from_ref(&folder), None);
        assert!(recycled.error.is_none(), "{:?}", recycled.error);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("new.txt"), b"new").unwrap();
        let restored = restore(&recycled.binned);
        assert!(restored.error.is_none(), "{:?}", restored.error);
        let [(0, back)] = restored.done.as_slice() else {
            panic!("expected one restored folder: {restored:?}");
        };
        assert_eq!(back, &dir.path().join("sx-recycle-folder (2)"));
        assert!(back.join("old.txt").exists() && !back.join("new.txt").exists());
        assert!(folder.join("new.txt").exists() && !folder.join("old.txt").exists());
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
