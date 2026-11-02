use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("COM call failed: {0}")]
    Com(windows::core::Error),
    #[error("this thread is already in the multi-threaded apartment")]
    Apartment,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("cannot open the item: {source}")]
    Open {
        path: PathBuf,
        source: windows::core::Error,
    },
    #[error("cannot start a shell worker thread: {0}")]
    Thread(std::io::Error),
    #[error("the shell worker stopped before answering")]
    WorkerGone,
    #[error("the operation was cancelled")]
    Aborted,
    #[error("the item is not in the Recycle Bin")]
    NotRecycled(PathBuf),
    #[error("{0}")]
    Io(std::io::Error),
}

impl ShellError {
    /// The Win32 error code (`ERROR_*`) behind a failed open, when there is one.
    pub fn win32_code(&self) -> Option<u32> {
        match self {
            Self::Open { source, .. } | Self::Com(source) => {
                sx_win::win32_code(source.code().0 as u32)
            }
            _ => None,
        }
    }

    /// The user cancelled a shell dialog (e.g. "Open with"), which is not a failure to report.
    pub fn is_cancelled(&self) -> bool {
        // HRESULT_FROM_WIN32(ERROR_CANCELLED)
        const CANCELLED: u32 = 0x8007_04C7;
        matches!(self, Self::Open { source, .. } if source.code().0 as u32 == CANCELLED)
    }
}
