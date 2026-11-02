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
use windows::core::PCWSTR;

use crate::error::WinError;
use crate::path::win32_path;

/// Entries per batch (ARCHITECTURE.md §8.1).
pub const BATCH: usize = 512;
/// The first batch is handed out early so the first rows (and a cancel check) come quickly.
pub const FIRST_BATCH: usize = 64;

/// `IsReparseTagNameSurrogate`: the tag names another item (junction, symlink, WSL link, …).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// One directory entry as Windows reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    /// UTF-8 name for display. Names that are not valid UTF-16 (a lone surrogate, possible on
    /// SMB/WSL volumes) are rendered lossily here and kept exactly in `exact`.
    pub name: String,
    /// The exact UTF-16 name when `name` is lossy. Paths must be built from this, never from
    /// `name`: two different lossy names can render identically.
    pub exact: Option<Box<[u16]>>,
    pub kind: EntryKind,
    /// `FILE_ATTRIBUTE_*`.
    pub attrs: u32,
    /// Reparse tag (`IO_REPARSE_TAG_*`) when `attrs` has `FILE_ATTRIBUTE_REPARSE_POINT`, else 0.
    pub reparse_tag: u32,
    pub size: u64,
    /// FILETIME (100 ns since 1601) of the last write.
    pub modified: i64,
    pub created: i64,
}

impl RawEntry {
    pub fn is_lossy(&self) -> bool {
        self.exact.is_some()
    }
}

/// Lists `dir` (an absolute path), calling `sink` with batches of entries: [`FIRST_BATCH`]
/// first, then up to [`BATCH`]. Returning `ControlFlow::Break` from `sink` stops early
/// (navigation away, cancel). Entries fetched before an error are still handed out.
///
/// Blocking: a sleeping NAS or a cloud folder can take seconds. Never call it on the UI thread.
pub fn enumerate(
    dir: &Path,
    mut sink: impl FnMut(&[RawEntry]) -> ControlFlow<()>,
) -> Result<(), WinError> {
    let pattern = win32_path(&dir.join("*"))?;
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
        // (A missing folder reports ERROR_PATH_NOT_FOUND instead.)
        Err(err) if err.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(()),
        Err(source) => {
            return Err(WinError::Enumerate {
                path: dir.to_path_buf(),
                source,
            });
        }
    };

    let mut batch = Vec::with_capacity(FIRST_BATCH);
    let mut limit = FIRST_BATCH;
    loop {
        if let Some(entry) = convert(&data) {
            batch.push(entry);
            if batch.len() == limit {
                if sink(&batch).is_break() {
                    return Ok(());
                }
                batch.clear();
                limit = BATCH;
            }
        }
        // SAFETY: `handle` is a live find handle and `data` a valid out-parameter.
        if let Err(err) = unsafe { FindNextFileW(handle.0, &raw mut data) } {
            if err.code() == ERROR_NO_MORE_FILES.to_hresult() {
                break;
            }
            if !batch.is_empty() {
                let _ = sink(&batch);
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
    let (name, exact) = match String::from_utf16(wide) {
        Ok(name) => (name, None),
        Err(_) => (String::from_utf16_lossy(wide), Some(Box::from(wide))),
    };
    let attrs = data.dwFileAttributes;
    let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    // For reparse points, dwReserved0 carries the reparse tag.
    let reparse_tag = if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        data.dwReserved0
    } else {
        0
    };
    // Name surrogates (junctions, mount points, symlinks, WSL links) point elsewhere: recursive
    // tools must not follow them (SECURITY.md §2.7). Cloud placeholders and dedup files are not
    // surrogates and behave like plain items.
    let kind = match (reparse_tag & NAME_SURROGATE != 0, is_dir) {
        (true, true) => EntryKind::Junction,
        (true, false) => EntryKind::Symlink,
        (false, true) => EntryKind::Dir,
        (false, false) => EntryKind::File,
    };
    let filetime = |high: u32, low: u32| ((u64::from(high) << 32) | u64::from(low)) as i64;
    Some(RawEntry {
        name,
        exact,
        kind,
        attrs,
        reparse_tag,
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::windows::ffi::OsStringExt as _;

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
        assert!(
            entries
                .iter()
                .any(|e| e.name == "日本語.txt" && !e.is_lossy())
        );
    }

    #[test]
    fn batches_and_early_stop() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(FIRST_BATCH + BATCH + 10) {
            fs::write(dir.path().join(format!("f{i}")), b"").unwrap();
        }
        let mut sizes = Vec::new();
        enumerate(dir.path(), |batch| {
            sizes.push(batch.len());
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(sizes, [FIRST_BATCH, BATCH, 10]);
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
        let err = enumerate(&dir.path().join("nope"), |_| ControlFlow::Continue(())).unwrap_err();
        assert_eq!(err.io_kind(), std::io::ErrorKind::NotFound);
        let mut deep = dir.path().to_path_buf();
        while deep.as_os_str().len() < 300 {
            deep.push("a-long-folder-name-for-max-path-tests");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), b"x").unwrap();
        assert_eq!(list(&deep).len(), 1);
    }

    #[test]
    fn relative_paths_are_rejected_not_resolved_against_the_cwd() {
        for path in ["C:", ".", "relative"] {
            let err = enumerate(Path::new(path), |_| ControlFlow::Continue(())).unwrap_err();
            assert!(matches!(err, WinError::RelativePath), "{path}");
        }
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, b"").unwrap();
        let err = enumerate(&file, |_| ControlFlow::Continue(())).unwrap_err();
        assert_ne!(err.io_kind(), std::io::ErrorKind::Other, "{err:?}");
    }

    #[test]
    fn lossy_names_keep_their_exact_form() {
        let dir = tempfile::tempdir().unwrap();
        // "lone" + an unpaired surrogate: valid on NTFS, not valid Unicode.
        let wide = [108, 111, 110, 101, 0xD800];
        let lone = dir.path().join(OsString::from_wide(&wide));
        fs::write(&lone, b"surrogate").unwrap();
        let entries = list(dir.path());
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(entry.is_lossy());
        assert_eq!(entry.exact.as_deref(), Some(&wide[..]));
    }

    #[test]
    fn trailing_dot_folders_are_listed_exactly() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("foo")).unwrap();
        fs::write(dir.path().join("foo").join("in_plain.txt"), b"").unwrap();
        // Win32 strips a trailing dot, so `foo.` can only be created through a verbatim path.
        let dotted = format!(r"\\?\{}\foo.", dir.path().display());
        fs::create_dir(&dotted).unwrap();
        fs::write(format!(r"{dotted}\in_dotted.txt"), b"").unwrap();
        let inside = list(&dir.path().join("foo."));
        assert_eq!(inside.len(), 1);
        assert_eq!(inside[0].name, "in_dotted.txt");
    }

    #[test]
    fn junctions_are_name_surrogates() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let entries = list(dir.path());
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Junction);
        assert_ne!(link.reparse_tag, 0);
    }
}
