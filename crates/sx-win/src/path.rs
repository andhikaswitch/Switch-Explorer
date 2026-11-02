//! Paths handed to Win32 (SYSTEM-INTEGRATION.md §15).
//!
//! Our own file-system calls always use the verbatim form (`\\?\C:\…`, `\\?\UNC\server\share\…`)
//! of a lexically normalised absolute path. Verbatim paths skip Win32's own normalisation, so:
//! - there is no `MAX_PATH` limit;
//! - names ending in a dot or a space (which Win32 would strip, opening a *different* item)
//!   reach the file system exactly as they were listed.
//!
//! Relative and drive-relative paths (`docs`, `C:`, `\docs`) are rejected: they would resolve
//! against the process-wide current directory, which other code can change at any time. Display
//! always uses the plain form.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Component, Path, Prefix};

use crate::error::WinError;

/// The NUL-terminated verbatim UTF-16 form of an absolute path:
/// - `/` becomes `\`, repeated separators collapse, `.` is dropped and `..` pops one component
///   (never above the root);
/// - paths that are already verbatim (`\\?\`) or device paths (`\\.\`) pass through unchanged.
pub fn win32_path(path: &Path) -> Result<Vec<u16>, WinError> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(WinError::RelativePath);
    };
    let mut out: Vec<u16> = match prefix.kind() {
        Prefix::Verbatim(_)
        | Prefix::VerbatimDisk(_)
        | Prefix::VerbatimUNC(..)
        | Prefix::DeviceNS(_) => return to_wide(path.as_os_str()),
        Prefix::Disk(letter) => {
            let mut head: Vec<u16> = r"\\?\".encode_utf16().collect();
            head.extend([u16::from(letter), u16::from(b':')]);
            head
        }
        Prefix::UNC(server, share) => {
            let mut head: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
            head.extend(server.encode_wide());
            head.push(u16::from(b'\\'));
            head.extend(share.encode_wide());
            head
        }
    };
    // `C:` without a root is relative to that drive's current directory.
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(WinError::RelativePath);
    }
    let mut parts: Vec<&OsStr> = Vec::new();
    for component in components {
        match component {
            Component::Normal(name) => parts.push(name),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.push(u16::from(b'\\'));
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(u16::from(b'\\'));
        }
        out.extend(part.encode_wide());
    }
    if out.contains(&0) {
        return Err(WinError::NulInPath);
    }
    out.push(0);
    Ok(out)
}

/// NUL-terminated UTF-16 for Win32 calls. A string with an interior NUL is rejected rather than
/// silently truncated to a different target.
pub fn to_wide(s: &OsStr) -> Result<Vec<u16>, WinError> {
    let mut wide: Vec<u16> = s.encode_wide().collect();
    if wide.contains(&0) {
        return Err(WinError::NulInPath);
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w32(path: &str) -> Result<String, WinError> {
        win32_path(Path::new(path)).map(|w| String::from_utf16_lossy(&w[..w.len() - 1]))
    }

    #[test]
    fn drive_paths_are_normalised_and_verbatim() {
        assert_eq!(w32(r"C:\").unwrap(), r"\\?\C:\");
        assert_eq!(w32(r"C:\Users\").unwrap(), r"\\?\C:\Users");
        assert_eq!(
            w32("C:/Users//Raka/./x/../Docs").unwrap(),
            r"\\?\C:\Users\Raka\Docs"
        );
        assert_eq!(w32(r"C:\..\..\a").unwrap(), r"\\?\C:\a");
        // A trailing dot is part of the name and must survive.
        assert_eq!(w32(r"C:\data\foo.").unwrap(), r"\\?\C:\data\foo.");
    }

    #[test]
    fn unc_verbatim_and_device_paths() {
        assert_eq!(
            w32(r"\\server\share\docs\a.txt").unwrap(),
            r"\\?\UNC\server\share\docs\a.txt"
        );
        assert_eq!(w32(r"\\?\C:\x\..\y").unwrap(), r"\\?\C:\x\..\y");
        assert_eq!(w32(r"\\.\C:\x").unwrap(), r"\\.\C:\x");
        let long = format!(r"C:\{}", "a".repeat(300));
        assert!(w32(&long).unwrap().starts_with(r"\\?\C:\aaa"));
    }

    #[test]
    fn relative_paths_and_nuls_are_rejected() {
        for path in ["docs", "C:", "C:docs", r"\docs", "."] {
            assert!(matches!(w32(path), Err(WinError::RelativePath)), "{path}");
        }
        assert!(matches!(w32("C:\\a\0b"), Err(WinError::NulInPath)));
        assert!(matches!(
            to_wide(OsStr::new("a\0b")),
            Err(WinError::NulInPath)
        ));
        assert_eq!(to_wide(OsStr::new("ab")).unwrap(), [97, 98, 0]);
    }
}
