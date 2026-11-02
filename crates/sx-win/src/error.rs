use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WinError {
    #[error("cannot list {path}: {source}")]
    Enumerate {
        path: PathBuf,
        source: windows::core::Error,
    },
}

impl WinError {
    /// The error as a portable kind (not found, permission denied, …) for user-facing messages.
    pub fn io_kind(&self) -> std::io::ErrorKind {
        let Self::Enumerate { source, .. } = self;
        let hr = source.code().0 as u32;
        // HRESULT_FROM_WIN32 wraps Win32 codes as 0x8007xxxx.
        if hr & 0xFFFF_0000 == 0x8007_0000 {
            std::io::Error::from_raw_os_error((hr & 0xFFFF) as i32).kind()
        } else {
            std::io::ErrorKind::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_win32_codes() {
        let err = WinError::Enumerate {
            path: PathBuf::from(
                r"C:
ope",
            ),
            // ERROR_ACCESS_DENIED
            source: windows::core::Error::from_hresult(windows::core::HRESULT(
                0x8007_0005_u32 as i32,
            )),
        };
        assert_eq!(err.io_kind(), std::io::ErrorKind::PermissionDenied);
    }
}
