//! Loading one folder into a [`DirSnapshot`]. Blocking: run it on a background executor
//! (CLAUDE.md rule 1); `cancel` stops it between batches when the user navigates away.

use std::io::ErrorKind;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;

use crate::snapshot::{DirSnapshot, SortSpec, ViewOptions};

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("folder not found: {0}")]
    NotFound(PathBuf),
    #[error("access denied: {0}")]
    AccessDenied(PathBuf),
    #[error(transparent)]
    Other(sx_win::WinError),
    #[error("cancelled")]
    Cancelled,
}

/// Enumerates and sorts `dir`.
pub fn load(
    dir: &Path,
    sort: SortSpec,
    view: ViewOptions,
    cancel: &AtomicBool,
) -> Result<DirSnapshot, LoadError> {
    let mut snapshot = DirSnapshot::new(sort, view);
    let listed = sx_win::enumerate(dir, |batch| {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        snapshot.push_batch(batch);
        ControlFlow::Continue(())
    });
    if let Err(err) = listed {
        return Err(match err.io_kind() {
            ErrorKind::NotFound => LoadError::NotFound(dir.to_path_buf()),
            ErrorKind::PermissionDenied => LoadError::AccessDenied(dir.to_path_buf()),
            _ => LoadError::Other(err),
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(LoadError::Cancelled);
    }
    snapshot.sort(sort);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_sorted_with_folders_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.txt", "a10.txt", "a2.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("zeta")).unwrap();
        let snap = load(
            dir.path(),
            SortSpec::default(),
            ViewOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let names: Vec<_> = (0..snap.visible_len())
            .map(|r| snap.row(r).unwrap().name.to_string())
            .collect();
        assert_eq!(names, ["zeta", "a2.txt", "a10.txt", "b.txt"]);
    }

    #[test]
    fn missing_folder_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(
            &dir.path().join("missing"),
            SortSpec::default(),
            ViewOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, LoadError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn cancel_stops_the_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"").unwrap();
        let err = load(
            dir.path(),
            SortSpec::default(),
            ViewOptions::default(),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(matches!(err, LoadError::Cancelled));
    }
}
