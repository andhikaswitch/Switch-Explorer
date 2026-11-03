//! Folder change notifications (`FindFirstChangeNotificationW`) for the open folder
//! (FEATURES.md, `perf.watcher`). The watcher only says "something changed"; the caller reloads
//! (debounced), which keeps it cheap and correct for any kind of change.
//!
//! A watched folder holds handles on its drive. So that a USB drive can still be ejected, the
//! watcher also registers for the drive's removal (`CM_Register_Notification` on a handle to the
//! folder). When Windows asks to remove the drive, the watcher closes its handles before the
//! callback returns. If the removal goes ahead the watch is over; if another app vetoes it, the
//! watcher reports one change and ends, so the caller reloads and starts a new watch.
//!
//! Everything that touches the disk or the Plug and Play service runs on the watcher's own
//! thread: starting a watch never blocks the caller, even on a slow network folder.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE, CM_NOTIFY_ACTION_DEVICEREMOVEPENDING,
    CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_0, CM_NOTIFY_FILTER_0_1,
    CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE, CM_Register_Notification, CM_Unregister_Notification,
    CR_SUCCESS, HCMNOTIFICATION,
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
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
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
    /// Another app vetoed the removal: the drive stays (set by the callback).
    kept: Event,
    /// The watcher thread is still watching (or starting to).
    running: AtomicBool,
}

impl Signals {
    fn new() -> windows::core::Result<Self> {
        Ok(Self {
            stop: Event::new()?,
            release: Event::new()?,
            released: Event::new()?,
            kept: Event::new()?,
            running: AtomicBool::new(true),
        })
    }
}

/// A change notification handle, closed on drop.
struct Change(HANDLE);

impl Drop for Change {
    fn drop(&mut self) {
        // SAFETY: the handle came from FindFirstChangeNotificationW and is closed once.
        let _ = unsafe { FindCloseChangeNotification(self.0) };
    }
}

/// The removal registration: an open handle on the folder (what the registration watches),
/// the registration, and the signals its callback uses (kept alive until it is unregistered).
/// Dropping it closes the handle and unregisters; it lives on the watcher thread, never in the
/// callback (unregistering there would deadlock).
struct Removal {
    folder: HANDLE,
    notify: HCMNOTIFICATION,
    _signals: Arc<Signals>,
}

impl Removal {
    /// `None` where the drive gives no removal notifications (network shares): those are not
    /// ejected, so there is nothing to release for.
    fn register(path: &[u16], signals: &Arc<Signals>) -> Option<Self> {
        // SAFETY: `path` is NUL-terminated; a directory handle with full sharing (it must not
        // get in anyone's way), closed by `close_folder`.
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
        // SAFETY: the context points into `signals`; the returned `Removal` holds a reference
        // to them until after CM_Unregister_Notification has returned, after which no
        // callback runs.
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
        Some(Self {
            folder,
            notify,
            _signals: signals.clone(),
        })
    }

    /// Closes the folder handle (idempotent); the registration stays until drop.
    fn close_folder(&mut self) {
        if !self.folder.is_invalid() {
            // SAFETY: our own handle, closed once (then marked invalid).
            let _ = unsafe { CloseHandle(self.folder) };
            self.folder = HANDLE::default();
        }
    }
}

impl Drop for Removal {
    fn drop(&mut self) {
        self.close_folder();
        // SAFETY: our registration, unregistered once, never from its own callback. It waits
        // for a running callback to return, then no callback runs and the signals may go.
        let _ = unsafe { CM_Unregister_Notification(self.notify) };
    }
}

/// The drive of a watched folder is about to go (or stays after all). Runs on a system thread
/// pool thread.
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
    let vetoed = action == CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED;
    if (removing || vetoed) && !context.is_null() {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: the context is the `Signals` of a live registration (see `register`).
            let signals = unsafe { &*context.cast::<Signals>() };
            if vetoed {
                signals.kept.set();
                return;
            }
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

impl DirWatcher {
    /// Whether it still watches: `false` once the folder could not be watched, went away, or
    /// its drive was removed. A caller that reloads the folder starts a new watcher then.
    pub fn is_running(&self) -> bool {
        self.signals.running.load(Ordering::Relaxed)
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        self.signals.stop.set();
    }
}

/// Clears `running` when the watcher thread ends, however it ends.
struct Running(Arc<Signals>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Relaxed);
    }
}

/// Calls `on_change` on a dedicated thread every time the folder's entries change, until the
/// returned watcher is dropped, `on_change` returns `false`, the folder goes away, or its drive
/// is removed. Not recursive. Returns at once: the folder is opened on the watcher thread, and
/// one that cannot be watched only makes [`DirWatcher::is_running`] turn `false`.
pub fn watch(
    dir: &Path,
    on_change: impl FnMut() -> bool + Send + 'static,
) -> Result<DirWatcher, WinError> {
    let path = win32_path(dir)?;
    let signals = Arc::new(Signals::new().map_err(|source| WinError::Watch {
        path: dir.to_path_buf(),
        source,
    })?);
    let thread_signals = signals.clone();
    std::thread::Builder::new()
        .name("sx-watch".into())
        .spawn(move || run(&path, &thread_signals, on_change))
        .map_err(|_| WinError::Thread)?;
    Ok(DirWatcher { signals })
}

/// How a watch loop ended.
enum End {
    /// Dropped, the folder went away, the wait failed, or the caller asked to stop.
    Done,
    /// The drive is being removed.
    Released,
}

fn run(path: &[u16], signals: &Arc<Signals>, mut on_change: impl FnMut() -> bool) {
    let _running = Running(signals.clone());
    // SAFETY: `path` is NUL-terminated; the handle is owned by `Change` below.
    let change = match unsafe { FindFirstChangeNotificationW(PCWSTR(path.as_ptr()), false, FILTER) }
    {
        Ok(handle) => Change(handle),
        Err(err) => {
            tracing::debug!(error = %err, "folder watch unavailable");
            return;
        }
    };
    let mut removal = Removal::register(path, signals);
    // Lowest index wins when several are signalled: stop and release before changes.
    let handles = [signals.stop.0, signals.release.0, change.0];
    let end = loop {
        // SAFETY: all handles stay alive for the loop (owned by this thread).
        let waited = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        match waited.0.wrapping_sub(WAIT_OBJECT_0.0) {
            0 => break End::Done,
            1 => break End::Released,
            2 => {}
            code => {
                tracing::debug!(code, "folder watch wait failed");
                break End::Done;
            }
        }
        if !on_change() {
            break End::Done;
        }
        // SAFETY: re-arms the notification on our own handle.
        if unsafe { FindNextChangeNotification(change.0) }.is_err() {
            break End::Done; // The folder went away.
        }
    };
    // Close every handle on the drive before a waiting removal callback returns.
    drop(change);
    if let Some(removal) = removal.as_mut() {
        removal.close_folder();
    }
    signals.released.set();
    if matches!(end, End::Released) {
        // Park until the watcher is dropped, or the removal is vetoed and the drive stays.
        let parked = [signals.stop.0, signals.kept.0];
        // SAFETY: both events are owned by `signals`, alive for the wait.
        let waited = unsafe { WaitForMultipleObjects(&parked, false, INFINITE) };
        if waited.0 == WAIT_OBJECT_0.0 + 1 {
            // Unregister first, then tell the caller: it reloads and starts a new watch.
            drop(removal.take());
            signals.running.store(false, Ordering::Relaxed);
            on_change();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    /// Polls until `done` holds, at most 5 s.
    fn eventually(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// What Windows does around a removal: the callback for `action` with the watcher's signals.
    fn notify(watcher: &DirWatcher, action: CM_NOTIFY_ACTION) -> u32 {
        // SAFETY: the context is the watcher's live `Signals`, as in a real registration.
        unsafe {
            on_removal(
                HCMNOTIFICATION::default(),
                Arc::as_ptr(&watcher.signals).cast(),
                action,
                std::ptr::null(),
                0,
            )
        }
    }

    #[test]
    fn reports_changes_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.path(), move || tx.send(()).is_ok()).unwrap();
        // The folder is opened on the watcher thread; keep writing until it reports.
        let mut n = 0;
        let seen = eventually(|| {
            n += 1;
            std::fs::write(dir.path().join(format!("new{n}.txt")), b"x").is_ok()
                && rx.recv_timeout(Duration::from_millis(100)).is_ok()
        });
        assert!(seen);
        drop(watcher);
        // After drop the thread exits and drops the sender: the channel disconnects.
        assert!(eventually(|| matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        )));
    }

    #[test]
    fn a_removal_request_releases_the_drive() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.path(), move || tx.send(()).is_ok()).unwrap();
        // The callback returns only once the watcher has closed its handles.
        assert_eq!(
            notify(&watcher, CM_NOTIFY_ACTION_DEVICEQUERYREMOVE),
            ERROR_SUCCESS.0,
            "never vetoes the removal"
        );
        // SAFETY: a zero-timeout check of an event the watcher owns.
        let released = unsafe { WaitForSingleObject(watcher.signals.released.0, 0) };
        assert_eq!(released, WAIT_OBJECT_0);
        // The drive is gone as far as the watch knows: changes are not reported.
        std::fs::write(dir.path().join("after.txt"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_vetoed_removal_ends_the_watch_with_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.path(), move || tx.send(()).is_ok()).unwrap();
        notify(&watcher, CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
        while rx.try_recv().is_ok() {}
        notify(&watcher, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        // One change, so the caller reloads, and the watcher says it has ended.
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(eventually(|| !watcher.is_running()));
    }

    #[test]
    fn local_drives_give_removal_notifications() {
        let dir = tempfile::tempdir().unwrap();
        let path = win32_path(dir.path()).unwrap();
        let signals = Arc::new(Signals::new().unwrap());
        let removal = Removal::register(&path, &signals);
        assert!(removal.is_some());
        drop(removal);
        assert_eq!(Arc::strong_count(&signals), 1, "unregistered and released");
    }

    #[test]
    fn a_missing_folder_ends_the_watch() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = watch(&dir.path().join("nope"), || true).unwrap();
        assert!(eventually(|| !watcher.is_running()));
    }
}
