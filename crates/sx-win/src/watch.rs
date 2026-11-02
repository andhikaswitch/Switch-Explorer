//! Folder change notifications (`FindFirstChangeNotificationW`) for the open folder
//! (FEATURES.md, `perf.watcher`). The watcher only says "something changed"; the caller reloads
//! (debounced), which keeps it cheap and correct for any kind of change.

use std::path::Path;
use std::sync::Arc;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::PCWSTR;

use crate::error::WinError;
use crate::path::win32_path;

const FILTER: FILE_NOTIFY_CHANGE = FILE_NOTIFY_CHANGE(
    FILE_NOTIFY_CHANGE_FILE_NAME.0
        | FILE_NOTIFY_CHANGE_DIR_NAME.0
        | FILE_NOTIFY_CHANGE_ATTRIBUTES.0
        | FILE_NOTIFY_CHANGE_SIZE.0
        | FILE_NOTIFY_CHANGE_LAST_WRITE.0,
);

/// An event handle closed on drop.
#[derive(Debug)]
struct Event(HANDLE);

// SAFETY: event handles may be signalled and closed from any thread.
unsafe impl Send for Event {}
// SAFETY: as above.
unsafe impl Sync for Event {}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// A change notification handle, closed on drop.
struct Change(HANDLE);

// SAFETY: the handle is moved to the watcher thread, which is its only user from then on.
unsafe impl Send for Change {}

impl Drop for Change {
    fn drop(&mut self) {
        // SAFETY: the handle came from FindFirstChangeNotificationW and is closed once.
        let _ = unsafe { FindCloseChangeNotification(self.0) };
    }
}

/// Watches one folder until dropped. Dropping signals the watcher thread, which exits on its
/// own; nothing waits for it, so dropping never blocks.
#[derive(Debug)]
pub struct DirWatcher {
    stop: Arc<Event>,
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        // SAFETY: signals our own event; the watcher thread holds another reference.
        let _ = unsafe { SetEvent(self.stop.0) };
    }
}

/// Calls `on_change` on a dedicated thread every time the folder's entries change, until the
/// returned watcher is dropped or `on_change` returns `false`. Not recursive.
pub fn watch(
    dir: &Path,
    mut on_change: impl FnMut() -> bool + Send + 'static,
) -> Result<DirWatcher, WinError> {
    let path = win32_path(dir)?;
    // SAFETY: `path` is NUL-terminated; the handle is owned by `Change` below.
    let change = unsafe { FindFirstChangeNotificationW(PCWSTR(path.as_ptr()), false, FILTER) }
        .map_err(|source| WinError::Watch {
            path: dir.to_path_buf(),
            source,
        })?;
    let change = Change(change);
    // SAFETY: an unnamed manual-reset event owned by `Event`.
    let stop = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
        .map(|h| Arc::new(Event(h)))
        .map_err(|source| WinError::Watch {
            path: dir.to_path_buf(),
            source,
        })?;
    let thread_stop = stop.clone();
    let spawned = std::thread::Builder::new()
        .name("sx-watch".into())
        .spawn(move || {
            // Move the owners (not just their handle fields) into the thread.
            let (change, stop) = (change, thread_stop);
            // Stop first: when both are signalled, the lowest index wins.
            let handles = [stop.0, change.0];
            loop {
                // SAFETY: both handles stay alive for the loop (owned by this thread).
                let waited = unsafe { WaitForMultipleObjects(&handles, false, u32::MAX) };
                if waited.0 != WAIT_OBJECT_0.0 + 1 {
                    return; // Stop requested, or the wait failed.
                }
                if !on_change() {
                    return;
                }
                // SAFETY: re-arms the notification on our own handle.
                if unsafe { FindNextChangeNotification(change.0) }.is_err() {
                    return; // The folder went away.
                }
            }
        });
    if spawned.is_err() {
        return Err(WinError::Thread);
    }
    Ok(DirWatcher { stop })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn reports_changes_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.path(), move || tx.send(()).is_ok()).unwrap();
        std::fs::write(dir.path().join("new.txt"), b"x").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(watcher);
        // Let the thread see the stop, then forget notifications it sent before that.
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        // After drop the thread exits; further changes are not reported (the sender is dropped
        // with the thread, so the channel disconnects).
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.path().join("later.txt"), b"x").unwrap();
        let after = rx.recv_timeout(Duration::from_millis(500));
        assert!(after.is_err());
    }

    #[test]
    fn missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(watch(&dir.path().join("nope"), || true).is_err());
    }
}
