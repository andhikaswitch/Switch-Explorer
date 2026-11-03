//! Folder change notifications (`FindFirstChangeNotificationW`) for the open folder
//! (FEATURES.md, `perf.watcher`). The watcher only says "something changed"; the caller reloads
//! (debounced), which keeps it cheap and correct for any kind of change.
//!
//! A watched folder holds handles on its drive. So that a USB drive can still be ejected, the
//! watcher also registers for the drive's removal (`CM_Register_Notification` on a handle to the
//! folder). When Windows asks to remove the drive, the watcher closes its handles before the
//! callback returns and stops; the listing stays on screen until the next refresh.

use std::path::Path;
use std::sync::Arc;

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE,
    CM_NOTIFY_ACTION_DEVICEREMOVEPENDING, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER,
    CM_NOTIFY_FILTER_0, CM_NOTIFY_FILTER_0_1, CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
    CM_Register_Notification, CM_Unregister_Notification, CR_SUCCESS, HCMNOTIFICATION,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FindCloseChangeNotification, FindFirstChangeNotificationW,
    FindNextChangeNotification, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{
    CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};
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

/// How long the removal callback waits for the watcher thread to close its handles. Windows
/// itself gives handle owners some seconds; the thread needs well under a millisecond.
const RELEASE_WAIT_MS: u32 = 2_000;

/// An event handle closed on drop.
#[derive(Debug)]
struct Event(HANDLE);

// SAFETY: event handles may be signalled and closed from any thread.
unsafe impl Send for Event {}
// SAFETY: as above.
unsafe impl Sync for Event {}

impl Event {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: an unnamed manual-reset event owned by the returned guard.
        unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map(Self)
    }

    fn set(&self) {
        // SAFETY: signals our own event.
        let _ = unsafe { SetEvent(self.0) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The signals shared by the watcher, its thread and the removal callback.
#[derive(Debug)]
struct Signals {
    /// The watcher was dropped.
    stop: Event,
    /// Windows wants to remove the drive (set by the callback).
    release: Event,
    /// The thread closed its handles on the drive (the callback may return).
    released: Event,
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

/// The removal registration: an open handle on the folder (what the registration watches) and
/// the registration itself.
struct Removal {
    folder: HANDLE,
    notify: HCMNOTIFICATION,
}

// SAFETY: moved to the watcher thread, its only user from then on.
unsafe impl Send for Removal {}

impl Removal {
    /// `None` where the drive gives no removal notifications (network shares): those are not
    /// ejected, so there is nothing to release for.
    fn register(path: &[u16], signals: &Arc<Signals>) -> Option<Self> {
        // SAFETY: `path` is NUL-terminated; a directory handle with full sharing (it must not
        // get in anyone's way), closed in `close`.
        let folder = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;
        let filter = CM_NOTIFY_FILTER {
            cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
            u: CM_NOTIFY_FILTER_0 {
                DeviceHandle: CM_NOTIFY_FILTER_0_1 { hTarget: folder },
            },
            ..Default::default()
        };
        let mut notify = HCMNOTIFICATION::default();
        // SAFETY: the context points into `signals`, which the watcher thread keeps alive until
        // after CM_Unregister_Notification has returned (no callback runs after that).
        let registered = unsafe {
            CM_Register_Notification(
                &raw const filter,
                Some(Arc::as_ptr(signals).cast()),
                Some(on_removal),
                &raw mut notify,
            )
        };
        if registered != CR_SUCCESS {
            // SAFETY: our own handle, not registered, closed once.
            let _ = unsafe { CloseHandle(folder) };
            return None;
        }
        Some(Self { folder, notify })
    }

    /// Closes the folder handle. Unregistering is separate (see the thread in [`watch`]).
    fn close_folder(&mut self) {
        if !self.folder.is_invalid() {
            // SAFETY: our own handle, closed once (then marked invalid).
            let _ = unsafe { CloseHandle(self.folder) };
            self.folder = HANDLE::default();
        }
    }

    fn unregister(mut self) {
        self.close_folder();
        // SAFETY: our registration, unregistered once, from the watcher thread (never from the
        // callback, which would deadlock). It waits for a running callback to return.
        let _ = unsafe { CM_Unregister_Notification(self.notify) };
    }
}

/// The drive of a watched folder is about to go. Runs on a system thread pool thread.
unsafe extern "system" fn on_removal(
    _notify: HCMNOTIFICATION,
    context: *const core::ffi::c_void,
    action: CM_NOTIFY_ACTION,
    _data: *const CM_NOTIFY_EVENT_DATA,
    _size: u32,
) -> u32 {
    let removing = [
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE,
        CM_NOTIFY_ACTION_DEVICEREMOVEPENDING,
        CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE,
    ]
    .contains(&action);
    if removing && !context.is_null() {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: the context is the `Signals` of a live registration (see `register`).
            let signals = unsafe { &*context.cast::<Signals>() };
            signals.release.set();
            // Windows goes on with the removal once this returns: wait until the handles are
            // closed.
            // SAFETY: waits on an event owned by `signals`.
            let _ = unsafe { WaitForSingleObject(signals.released.0, RELEASE_WAIT_MS) };
        });
    }
    // Never veto: the user asked to remove the drive.
    ERROR_SUCCESS.0
}

/// Watches one folder until dropped. Dropping signals the watcher thread, which exits on its
/// own; nothing waits for it, so dropping never blocks.
#[derive(Debug)]
pub struct DirWatcher {
    signals: Arc<Signals>,
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        self.signals.stop.set();
    }
}

/// Calls `on_change` on a dedicated thread every time the folder's entries change, until the
/// returned watcher is dropped, `on_change` returns `false`, or the folder's drive is removed.
/// Not recursive.
pub fn watch(
    dir: &Path,
    mut on_change: impl FnMut() -> bool + Send + 'static,
) -> Result<DirWatcher, WinError> {
    let fail = |source| WinError::Watch {
        path: dir.to_path_buf(),
        source,
    };
    let path = win32_path(dir)?;
    // SAFETY: `path` is NUL-terminated; the handle is owned by `Change` below.
    let change = unsafe { FindFirstChangeNotificationW(PCWSTR(path.as_ptr()), false, FILTER) }
        .map_err(fail)?;
    let change = Change(change);
    let signals = Arc::new(Signals {
        stop: Event::new().map_err(fail)?,
        release: Event::new().map_err(fail)?,
        released: Event::new().map_err(fail)?,
    });
    let removal = Removal::register(&path, &signals);
    let thread_signals = signals.clone();
    let spawned = std::thread::Builder::new()
        .name("sx-watch".into())
        .spawn(move || {
            // Move the owners (not just their handle fields) into the thread.
            let (change, signals, mut removal) = (change, thread_signals, removal);
            // Lowest index wins when several are signalled: stop and release before changes.
            let handles = [signals.stop.0, signals.release.0, change.0];
            loop {
                // SAFETY: all handles stay alive for the loop (owned by this thread).
                let waited = unsafe { WaitForMultipleObjects(&handles, false, u32::MAX) };
                if waited.0 != WAIT_OBJECT_0.0 + 2 {
                    if waited.0 > WAIT_OBJECT_0.0 + 2 {
                        tracing::debug!(code = waited.0, "folder watch wait failed");
                    }
                    break; // Stopped, the drive is going, or the wait failed.
                }
                if !on_change() {
                    break;
                }
                // SAFETY: re-arms the notification on our own handle.
                if unsafe { FindNextChangeNotification(change.0) }.is_err() {
                    break; // The folder went away.
                }
            }
            // Close every handle on the drive, let a waiting removal callback return, and only
            // then unregister (which waits for that callback).
            drop(change);
            if let Some(removal) = removal.as_mut() {
                removal.close_folder();
            }
            signals.released.set();
            if let Some(removal) = removal {
                removal.unregister();
            }
        });
    if spawned.is_err() {
        return Err(WinError::Thread);
    }
    Ok(DirWatcher { signals })
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
    fn a_removal_request_releases_the_drive() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.path(), move || tx.send(()).is_ok()).unwrap();
        // What Windows does when the user ejects the drive: the callback returns only once the
        // watcher has closed its handles.
        // SAFETY: the context is the watcher's live `Signals`, as in a real registration.
        let answer = unsafe {
            on_removal(
                HCMNOTIFICATION::default(),
                Arc::as_ptr(&watcher.signals).cast(),
                CM_NOTIFY_ACTION_DEVICEQUERYREMOVE,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(answer, ERROR_SUCCESS.0, "never vetoes the removal");
        // SAFETY: a zero-timeout check of an event the watcher owns.
        let released = unsafe { WaitForSingleObject(watcher.signals.released.0, 0) };
        assert_eq!(released, WAIT_OBJECT_0);
        // The watch has ended: changes are no longer reported.
        std::fs::write(dir.path().join("after.txt"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
    }

    #[test]
    fn local_drives_give_removal_notifications() {
        let dir = tempfile::tempdir().unwrap();
        let path = win32_path(dir.path()).unwrap();
        let signals = Arc::new(Signals {
            stop: Event::new().unwrap(),
            release: Event::new().unwrap(),
            released: Event::new().unwrap(),
        });
        let removal = Removal::register(&path, &signals).expect("registered");
        removal.unregister();
    }

    #[test]
    fn missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(watch(&dir.path().join("nope"), || true).is_err());
    }
}
