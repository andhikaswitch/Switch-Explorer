use std::io::ErrorKind;
use std::path::PathBuf;

use thiserror::Error;

/// Errors keep the path as a field but never in their message, so logging an error with `%err`
/// does not write a user path above `debug` level (SECURITY.md §8).
#[derive(Debug, Error)]
pub enum WinError {
    #[error("the path is not absolute")]
    RelativePath,
    #[error("the path contains a NUL character")]
    NulInPath,
    #[error("cannot list the folder: {source}")]
    Enumerate {
        path: PathBuf,
        source: windows::core::Error,
    },
}

impl WinError {
    /// The error as a portable kind (not found, permission denied, not a folder, …) for
    /// user-facing messages.
    pub fn io_kind(&self) -> ErrorKind {
        match self {
            Self::RelativePath | Self::NulInPath => ErrorKind::InvalidInput,
            Self::Enumerate { source, .. } => hresult_kind(source.code().0 as u32),
        }
    }
}

/// Maps `HRESULT_FROM_WIN32` codes (0x8007xxxx) through std's Win32 error table.
fn hresult_kind(hr: u32) -> ErrorKind {
    if hr & 0xFFFF_0000 == 0x8007_0000 {
        std::io::Error::from_raw_os_error((hr & 0xFFFF) as i32).kind()
    } else {
        ErrorKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_win32_codes() {
        // ERROR_ACCESS_DENIED, ERROR_PATH_NOT_FOUND, ERROR_DIRECTORY.
        assert_eq!(hresult_kind(0x8007_0005), ErrorKind::PermissionDenied);
        assert_eq!(hresult_kind(0x8007_0003), ErrorKind::NotFound);
        assert_eq!(hresult_kind(0x8007_010B), ErrorKind::NotADirectory);
        assert_eq!(hresult_kind(0x8000_4005), ErrorKind::Other);
    }

    #[test]
    fn messages_do_not_contain_the_path() {
        let err = WinError::Enumerate {
            path: PathBuf::from(r"C:\Users\Raka\secret"),
            source: windows::core::Error::from_hresult(windows::core::HRESULT(
                0x8007_0005_u32 as i32,
            )),
        };
        assert!(!err.to_string().contains("Raka"));
    }
}
