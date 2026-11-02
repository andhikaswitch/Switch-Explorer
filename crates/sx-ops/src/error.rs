use std::path::{Path, PathBuf};

use thiserror::Error;

/// Messages never contain the path (it stays a field), so errors can be logged at any level.
#[derive(Debug, Error)]
pub enum OpsError {
    #[error("{source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("an item with that name already exists")]
    Exists(PathBuf),
    #[error("a folder cannot be copied or moved into itself")]
    IntoItself(PathBuf),
    #[error("the copy does not match the original")]
    VerifyFailed(PathBuf),
    #[error("the item is not in the Recycle Bin")]
    NotInRecycleBin(PathBuf),
    #[error("the Recycle Bin refused the request: {0}")]
    Trash(String),
    #[error("cannot write the journal: {0}")]
    Journal(serde_json::Error),
    #[error("read-only mode is on")]
    ReadOnly,
}

impl OpsError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    /// The item the error is about, when there is one.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Io { path, .. }
            | Self::Exists(path)
            | Self::IntoItself(path)
            | Self::VerifyFailed(path)
            | Self::NotInRecycleBin(path) => Some(path),
            Self::Trash(_) | Self::Journal(_) | Self::ReadOnly => None,
        }
    }
}
