//! Operation progress on the window's taskbar button (`ITaskbarList3`, SYSTEM-INTEGRATION.md
//! §12, `ops.taskbar_progress`).
//!
//! The calls go to Explorer's process and wait for it, so they run on their own STA worker,
//! never the UI thread. Only the latest state matters: updates are coalesced, and a burst of
//! progress reports costs one call.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::{
    ITaskbarList3, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
};

use crate::error::ShellError;
use crate::worker::{Order, StaWorker};

/// What the taskbar button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarState {
    /// No progress (nothing running).
    Idle,
    /// Running, total unknown yet.
    Busy,
    /// `done` of `total` (any unit: items or bytes).
    Progress { done: u64, total: u64 },
}

/// The taskbar button of one window.
pub struct Taskbar {
    worker: StaWorker,
    latest: Arc<Mutex<Option<TaskbarState>>>,
    hwnd: isize,
}

impl std::fmt::Debug for Taskbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Taskbar").finish_non_exhaustive()
    }
}

thread_local! {
    /// The taskbar object of the worker thread, made on first use (STA: used only there).
    static LIST: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
}

fn list() -> Option<ITaskbarList3> {
    LIST.with_borrow_mut(|list| {
        if list.is_none() {
            // SAFETY: creates the shell's taskbar object in this worker's STA and initialises
            // it once; it is only used on this thread.
            let created = unsafe {
                CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                    .and_then(|taskbar| taskbar.HrInit().map(|()| taskbar))
            };
            match created {
                Ok(taskbar) => *list = Some(taskbar),
                Err(err) => tracing::debug!(error = %err, "taskbar progress unavailable"),
            }
        }
        list.clone()
    })
}

fn apply(hwnd: isize, state: TaskbarState) {
    let Some(taskbar) = list() else {
        return;
    };
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: plain calls on our taskbar object with the window's handle; a window without a
    // taskbar button (yet) only makes them fail.
    let result = unsafe {
        match state {
            TaskbarState::Idle => taskbar.SetProgressState(hwnd, TBPF_NOPROGRESS),
            TaskbarState::Busy => taskbar.SetProgressState(hwnd, TBPF_INDETERMINATE),
            TaskbarState::Progress { done, total } => taskbar
                .SetProgressState(hwnd, TBPF_NORMAL)
                .and_then(|()| taskbar.SetProgressValue(hwnd, done.min(total), total.max(1))),
        }
    };
    if let Err(err) = result {
        tracing::debug!(error = %err, "taskbar progress");
    }
}

impl Taskbar {
    /// The taskbar button of the window `hwnd` (a raw `HWND`).
    pub fn new(hwnd: isize) -> Result<Self, ShellError> {
        Ok(Self {
            worker: StaWorker::spawn("sx-taskbar", Order::Oldest)?,
            latest: Arc::default(),
            hwnd,
        })
    }

    /// Shows `state`. Returns at once; only the newest state reaches the taskbar.
    pub fn set(&self, state: TaskbarState) {
        let replaced = self
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(state)
            .is_some();
        if replaced {
            // A job that will pick this state up is already queued.
            return;
        }
        let (latest, hwnd) = (self.latest.clone(), self.hwnd);
        self.worker.run(move || {
            let state = latest.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(state) = state {
                apply(hwnd, state);
            }
        });
    }
}
