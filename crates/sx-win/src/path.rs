//! Long paths (SYSTEM-INTEGRATION.md §15): our own Win32 calls get the `\\?\` prefix when a path
//! could exceed `MAX_PATH`; display always uses the plain form.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

/// Paths at or above this length get the verbatim prefix (directory APIs stop at 248).
const PREFIX_FROM: usize = 248;

/// `C:\a\b` → `\\?\C:\a\b`, `\\server\share\x` → `\\?\UNC\server\share\x` when the path is long
/// and absolute. Relative or already verbatim paths are returned unchanged.
pub fn verbatim(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if text.len() < PREFIX_FROM || text.starts_with(r"\\?\") || !path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(unc) = text.strip_prefix(r"\\") {
        PathBuf::from(format!(r"\\?\UNC\{unc}"))
    } else {
        PathBuf::from(format!(r"\\?\{text}"))
    }
}

/// NUL-terminated UTF-16 for Win32 calls.
pub fn to_wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths_stay_plain() {
        assert_eq!(verbatim(Path::new(r"C:\Users")), PathBuf::from(r"C:\Users"));
        assert_eq!(verbatim(Path::new("relative")), PathBuf::from("relative"));
    }

    #[test]
    fn long_paths_get_prefixed() {
        let long = format!(r"C:\{}", "a".repeat(300));
        assert!(verbatim(Path::new(&long)).starts_with(r"\\?\C:\"));
        let unc = format!(r"\\server\share\{}", "b".repeat(300));
        assert!(
            verbatim(Path::new(&unc))
                .to_string_lossy()
                .starts_with(r"\\?\UNC\server\share\")
        );
        let already = format!(r"\\?\C:\{}", "c".repeat(300));
        assert_eq!(verbatim(Path::new(&already)), PathBuf::from(&already));
    }
}
