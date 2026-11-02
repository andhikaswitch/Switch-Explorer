//! Directory enumeration with `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)`:
//! attributes, size and times arrive with each name, so there is no per-file stat
//! (PERFORMANCE.md §4). Entries are handed out in batches so the first rows can render early.

use std::ops::ControlFlow;
use std::path::Path;

use sx_core::EntryKind;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH, FindClose,
    FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, WIN32_FIND_DATAW,
};
use windows::Win32::System::SystemServices::{IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK};
use windows::core::PCWSTR;

use crate::error::WinError;
use crate::path::{to_wide, verbatim};

/// Entries per batch (ARCHITECTURE.md §8.1).
pub const BATCH: usize = 512;

/// One directory entry as Windows reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    /// UTF-8 name; names that are not valid UTF-16 are converted lossily (`lossy = true`).
    pub name: String,
    pub lossy: bool,
    pub kind: EntryKind,
    /// `FILE_ATTRIBUTE_*`.
    pub attrs: u32,
    pub size: u64,
    /// FILETIME (100 ns since 1601) of the last write.
    pub modified: i64,
    pub created: i64,
}

/// Lists `dir`, calling `sink` with batches of up to [`BATCH`] entries; returning
/// `ControlFlow::Break` from `sink` stops early (navigation away, cancel).
pub fn enumerate(
    dir: &Path,
    mut sink: impl FnMut(&[RawEntry]) -> ControlFlow<()>,
) -> Result<(), WinError> {
    let pattern = to_wide(verbatim(&dir.join("*")).as_os_str());
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` is NUL-terminated and `data` is a valid out-parameter for this call.
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let handle = match handle {
        Ok(handle) => FindHandle(handle),
        // An empty volume root reports "not found": that is an empty listing, not an error.
        Err(err) if err.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(()),
        Err(source) => {
            return Err(WinError::Enumerate {
                path: dir.to_path_buf(),
                source,
            });
        }
    };

    let mut batch = Vec::with_capacity(BATCH);
    loop {
        if let Some(entry) = convert(&data) {
            batch.push(entry);
            if batch.len() == BATCH {
                if sink(&batch).is_break() {
                    return Ok(());
                }
                batch.clear();
            }
        }
        // SAFETY: `handle` is a live find handle and `data` a valid out-parameter.
        if let Err(err) = unsafe { FindNextFileW(handle.0, &raw mut data) } {
            if err.code() == ERROR_NO_MORE_FILES.to_hresult() {
                break;
            }
            return Err(WinError::Enumerate {
                path: dir.to_path_buf(),
                source: err,
            });
        }
    }
    if !batch.is_empty() {
        let _ = sink(&batch);
    }
    Ok(())
}

fn convert(data: &WIN32_FIND_DATAW) -> Option<RawEntry> {
    let len = data
        .cFileName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(data.cFileName.len());
    let wide = &data.cFileName[..len];
    if wide == [u16::from(b'.')] || wide == [u16::from(b'.'), u16::from(b'.')] {
        return None;
    }
    let (name, lossy) = match String::from_utf16(wide) {
        Ok(name) => (name, false),
        Err(_) => (String::from_utf16_lossy(wide), true),
    };
    let attrs = data.dwFileAttributes;
    let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    // For reparse points, dwReserved0 carries the reparse tag.
    let kind = if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        match data.dwReserved0 {
            tag if tag == IO_REPARSE_TAG_MOUNT_POINT => EntryKind::Junction,
            tag if tag == IO_REPARSE_TAG_SYMLINK && is_dir => EntryKind::Junction,
            tag if tag == IO_REPARSE_TAG_SYMLINK => EntryKind::Symlink,
            // Cloud placeholders (OneDrive) and other tags behave like plain items.
            _ if is_dir => EntryKind::Dir,
            _ => EntryKind::File,
        }
    } else if is_dir {
        EntryKind::Dir
    } else {
        EntryKind::File
    };
    let filetime = |high: u32, low: u32| ((u64::from(high) << 32) | u64::from(low)) as i64;
    Some(RawEntry {
        name,
        lossy,
        kind,
        attrs,
        size: (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow),
        modified: filetime(
            data.ftLastWriteTime.dwHighDateTime,
            data.ftLastWriteTime.dwLowDateTime,
        ),
        created: filetime(
            data.ftCreationTime.dwHighDateTime,
            data.ftCreationTime.dwLowDateTime,
        ),
    })
}

/// Closes the find handle on drop.
struct FindHandle(HANDLE);

impl Drop for FindHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from FindFirstFileExW and is closed exactly once.
        let _ = unsafe { FindClose(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn list(dir: &Path) -> Vec<RawEntry> {
        let mut all = Vec::new();
        enumerate(dir, |batch| {
            all.extend_from_slice(batch);
            ControlFlow::Continue(())
        })
        .unwrap();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    #[test]
    fn lists_files_and_folders_with_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("日本語.txt"), b"").unwrap();
        let entries = list(dir.path());
        assert_eq!(entries.len(), 3);
        let a = &entries[0];
        assert_eq!(
            (a.name.as_str(), a.kind, a.size),
            ("a.txt", EntryKind::File, 5)
        );
        assert!(a.modified > 0);
        assert!(
            entries
                .iter()
                .any(|e| e.name == "sub" && e.kind == EntryKind::Dir)
        );
        assert!(entries.iter().any(|e| e.name == "日本語.txt" && !e.lossy));
    }

    #[test]
    fn batches_and_early_stop() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(BATCH + 10) {
            fs::write(dir.path().join(format!("f{i}")), b"").unwrap();
        }
        let mut sizes = Vec::new();
        enumerate(dir.path(), |batch| {
            sizes.push(batch.len());
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(sizes, [BATCH, 10]);
        let mut calls = 0;
        enumerate(dir.path(), |_| {
            calls += 1;
            ControlFlow::Break(())
        })
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn missing_folder_is_an_error_and_long_paths_work() {
        let dir = tempfile::tempdir().unwrap();
        assert!(enumerate(&dir.path().join("nope"), |_| ControlFlow::Continue(())).is_err());
        let mut deep = dir.path().to_path_buf();
        while deep.as_os_str().len() < 300 {
            deep.push("a-long-folder-name-for-max-path-tests");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), b"x").unwrap();
        assert_eq!(list(&deep).len(), 1);
    }
}
