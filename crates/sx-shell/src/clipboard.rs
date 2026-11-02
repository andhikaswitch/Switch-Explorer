//! Copy/cut/paste of files through the clipboard, interoperable with File Explorer (FEATURES.md
//! F-OPS-01): `CF_HDROP` carries the paths and `Preferred DropEffect` says copy or move.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_MOVE};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows::core::w;

use crate::error::ShellError;

/// Another app may hold the clipboard for a moment; retry this many times.
const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY: Duration = Duration::from_millis(15);

/// Files on the clipboard and whether they were cut (move) or copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardFiles {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

fn preferred_drop_effect() -> u32 {
    // SAFETY: registers (or looks up) a named format with a static name.
    unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }
}

/// Holds the clipboard open; closed on drop.
struct Open;

impl Open {
    fn new(owner: Option<isize>) -> Result<Self, ShellError> {
        let owner = owner.map(|h| HWND(h as *mut core::ffi::c_void));
        let mut last = None;
        for _ in 0..OPEN_ATTEMPTS {
            // SAFETY: opens the clipboard for this thread; closed by Drop.
            match unsafe { OpenClipboard(owner) } {
                Ok(()) => return Ok(Self),
                Err(err) => last = Some(err),
            }
            std::thread::sleep(OPEN_RETRY);
        }
        Err(ShellError::Com(
            last.unwrap_or_else(windows::core::Error::from_thread),
        ))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: closes the clipboard opened in `new` on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

/// A movable global block filled by `fill`; freed unless the clipboard takes ownership.
fn global(len: usize, fill: impl FnOnce(&mut [u8])) -> Result<HGLOBAL, ShellError> {
    // SAFETY: allocates, locks, fills and unlocks a block we own; the slice covers exactly the
    // allocated length.
    unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, len).map_err(ShellError::Com)?;
        let ptr = GlobalLock(block).cast::<u8>();
        if ptr.is_null() {
            let _ = GlobalFree(Some(block));
            return Err(ShellError::Com(windows::core::Error::from_thread()));
        }
        fill(std::slice::from_raw_parts_mut(ptr, len));
        let _ = GlobalUnlock(block);
        Ok(block)
    }
}

fn set(format: u32, block: HGLOBAL) -> Result<(), ShellError> {
    // SAFETY: hands the block to the clipboard; on success the clipboard owns it, on failure we
    // free it.
    unsafe {
        if let Err(err) = SetClipboardData(format, Some(HANDLE(block.0))) {
            let _ = GlobalFree(Some(block));
            return Err(ShellError::Com(err));
        }
    }
    Ok(())
}

/// Puts files on the clipboard as Explorer does.
pub fn write(files: &ClipboardFiles, owner: Option<isize>) -> Result<(), ShellError> {
    // DROPFILES header, then NUL-terminated wide paths, then one more NUL.
    let mut wide: Vec<u16> = Vec::new();
    for path in &files.paths {
        wide.extend(path.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);
    let header = size_of::<DROPFILES>();
    let drop_files = global(header + wide.len() * 2, |bytes| {
        let info = DROPFILES {
            pFiles: header as u32,
            fWide: true.into(),
            ..Default::default()
        };
        // SAFETY: `bytes` holds at least `header` bytes; DROPFILES is plain data.
        unsafe { std::ptr::write_unaligned(bytes.as_mut_ptr().cast::<DROPFILES>(), info) };
        for (i, unit) in wide.iter().enumerate() {
            bytes[header + i * 2..header + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
    })?;
    let effect = if files.cut {
        DROPEFFECT_MOVE.0
    } else {
        DROPEFFECT_COPY.0
    };
    let _open = Open::new(owner)?;
    // SAFETY: the clipboard is open on this thread.
    unsafe { EmptyClipboard() }.map_err(ShellError::Com)?;
    set(u32::from(CF_HDROP.0), drop_files)?;
    let effect_block = global(4, |bytes| bytes.copy_from_slice(&effect.to_le_bytes()))?;
    set(preferred_drop_effect(), effect_block)
}

/// Files on the clipboard (from us or from Explorer), or `None` when it holds no files.
pub fn read() -> Result<Option<ClipboardFiles>, ShellError> {
    // SAFETY: a plain availability query.
    if unsafe { IsClipboardFormatAvailable(u32::from(CF_HDROP.0)) }.is_err() {
        return Ok(None);
    }
    let _open = Open::new(None)?;
    // SAFETY: the clipboard is open; the handles stay owned by the clipboard and are only read
    // while it is open.
    unsafe {
        let Ok(handle) = GetClipboardData(u32::from(CF_HDROP.0)) else {
            return Ok(None);
        };
        let drop = HDROP(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            paths.push(PathBuf::from(OsString::from_wide(&buf[..got.min(len)])));
        }
        let cut = GetClipboardData(preferred_drop_effect())
            .ok()
            .and_then(|h| {
                let block = HGLOBAL(h.0);
                let ptr = GlobalLock(block).cast::<u8>();
                if ptr.is_null() {
                    return None;
                }
                let mut value = [0u8; 4];
                std::ptr::copy_nonoverlapping(ptr, value.as_mut_ptr(), 4);
                let _ = GlobalUnlock(block);
                Some(u32::from_le_bytes(value))
            })
            .is_some_and(|effect| effect & DROPEFFECT_MOVE.0 != 0);
        Ok(Some(ClipboardFiles { paths, cut }))
    }
}

/// Clears the clipboard after a cut has been pasted (the files are no longer where it says).
pub fn clear(owner: Option<isize>) -> Result<(), ShellError> {
    let _open = Open::new(owner)?;
    // SAFETY: the clipboard is open on this thread.
    unsafe { EmptyClipboard() }.map_err(ShellError::Com)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replaces the user's clipboard, so it only runs on request: `cargo test -- --ignored`.
    #[test]
    #[ignore = "replaces the clipboard"]
    fn round_trip_with_explorer_formats() {
        let files = ClipboardFiles {
            paths: vec![
                PathBuf::from(r"C:\Users\Public\a.txt"),
                PathBuf::from(r"D:\写真\b c.jpg"),
            ],
            cut: true,
        };
        write(&files, None).unwrap();
        assert_eq!(read().unwrap(), Some(files));
        clear(None).unwrap();
        assert_eq!(read().unwrap(), None);
    }
}
