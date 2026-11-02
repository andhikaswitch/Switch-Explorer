//! Opening items with their default verb (`ShellExecuteExW`), as Explorer does on Enter or
//! double-click (FEATURES.md F-NAV-04).
//!
//! Each call runs on its own short-lived STA thread: `ShellExecuteEx` can block for seconds
//! (network paths, slow handlers, the "Open with" dialog), and the UI thread must not.

use std::path::{Path, PathBuf};

use futures::channel::oneshot;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::com::StaGuard;
use crate::error::ShellError;

/// Opens `path` with its default verb. `owner` is the raw handle of the window that owns any
/// dialog the shell shows. Resolves once the shell has handed the item over (or failed).
pub fn open(
    path: PathBuf,
    owner: Option<isize>,
) -> impl Future<Output = Result<(), ShellError>> + Send + 'static {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("sx-shell-exec".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(|| open_blocking(&path, owner))
                .unwrap_or(Err(ShellError::WorkerGone));
            let _ = tx.send(result);
        });
    async move {
        spawned.map_err(ShellError::Thread)?;
        rx.await.map_err(|_| ShellError::WorkerGone)?
    }
}

fn open_blocking(path: &Path, owner: Option<isize>) -> Result<(), ShellError> {
    let invalid = |err: sx_win::WinError| ShellError::InvalidPath(err.to_string());
    // ShellExecute does not accept verbatim paths; the plain form is fine here because the
    // path came from a listing of an absolute folder.
    let file = sx_win::to_wide(path.as_os_str()).map_err(invalid)?;
    let dir = match path.parent() {
        Some(parent) => sx_win::to_wide(parent.as_os_str()).map_err(invalid)?,
        None => vec![0],
    };
    let _com = StaGuard::new()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait for DDE conversations to finish before this thread (and its apartment) exits.
        fMask: SEE_MASK_NOASYNC,
        hwnd: HWND(owner.unwrap_or(0) as *mut core::ffi::c_void),
        lpFile: PCWSTR(file.as_ptr()),
        lpDirectory: PCWSTR(dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is fully initialised with its size; `file` and `dir` are NUL-terminated and
    // outlive the call; the COM apartment is held by `_com` on this thread.
    unsafe { ShellExecuteExW(&raw mut info) }.map_err(|source| ShellError::Open {
        path: path.to_path_buf(),
        source,
    })
}
