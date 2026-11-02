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
