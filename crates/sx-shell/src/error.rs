use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("COM call failed: {0}")]
    Com(windows::core::Error),
    #[error("this thread is already in the multi-threaded apartment")]
    Apartment,
    #[error("cannot open {path}: {source}")]
    Open {
        path: PathBuf,
        source: windows::core::Error,
    },
    #[error("cannot start a shell worker thread: {0}")]
    Thread(std::io::Error),
    #[error("the shell worker stopped before answering")]
    WorkerGone,
}

impl ShellError {
    /// The user cancelled a shell dialog (e.g. "Open with"), which is not a failure to report.
    pub fn is_cancelled(&self) -> bool {
        // HRESULT_FROM_WIN32(ERROR_CANCELLED)
        const CANCELLED: u32 = 0x8007_04C7;
        matches!(self, Self::Open { source, .. } if source.code().0 as u32 == CANCELLED)
    }
}
