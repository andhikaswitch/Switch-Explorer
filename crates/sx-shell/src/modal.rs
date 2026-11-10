//! The modal host (ADR-015): OS modal loops (`TrackPopupMenuEx`, `DoDragDrop`, shell dialogs) run
//! from our own message-only window, not from inside a GPUI task.
//!
//! A modal loop keeps pumping messages, so a GPUI task that starts one would run other tasks
//! inside itself; with GPUI's profiler that nesting panics, and it is fragile in any case.
//! Posting a wake-up to this window runs the job from the plain message loop, outside GPUI's
//! borrows and its executor, and the result comes back through a oneshot channel that a
//! foreground task awaits.
//!
//! Jobs wait in a thread-local queue (never as pointers inside messages), and only one runs at a
//! time: a wake-up that a nested loop dispatches leaves the queue alone, and the running job posts
//! again when it finishes. Everything here belongs to the UI thread, where [`init`] runs.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use futures::channel::oneshot;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_MESSAGE, PostMessageW, RegisterClassExW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WNDCLASSEXW,
};
use windows::core::w;

use crate::ShellError;

const WM_RUN_MODAL: u32 = WM_APP + 0x51;

type Job = Box<dyn FnOnce()>;

thread_local! {
    static HOST: Cell<isize> = const { Cell::new(0) };
    static QUEUE: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// Creates the host window. Call once on the UI thread at startup.
pub fn init() -> Result<(), ShellError> {
    create_host().map(|_| ())
}

/// Queues `job` to run on this (the UI) thread from the host window's message handler. If the
/// wake-up cannot be posted (no host on this thread: another thread, or `init` failed), or the
/// job panics, the receiver resolves to `Canceled`.
pub fn run<T: 'static>(job: impl FnOnce() -> T + 'static) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    QUEUE.with(|queue| {
        queue.borrow_mut().push_back(Box::new(move || {
            let _ = tx.send(job());
        }));
    });
    if let Err(err) = wake() {
        tracing::error!(error = %err, "modal host: cannot queue a job");
        // Dropping the queued job drops its sender: the awaiting task sees `Canceled`.
        QUEUE.with(|queue| queue.borrow_mut().pop_back());
    }
    rx
}

/// Runs `f`, turning a panic into `fallback`: panics must not unwind into the OS (callbacks,
/// window procedures).
pub(crate) fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        tracing::error!("panic in a shell callback");
        fallback
    })
}

fn wake() -> Result<(), ShellError> {
    // Only the thread that ran `init` has a host (and a message loop to run it).
    let Some(hwnd) = HOST.with(|h| (h.get() != 0).then(|| HWND(h.get() as *mut _))) else {
        return Err(ShellError::WorkerGone);
    };
    // SAFETY: posts a message without data to our own window.
    unsafe { PostMessageW(Some(hwnd), WM_RUN_MODAL, WPARAM(0), LPARAM(0)) }.map_err(ShellError::Com)
}

fn create_host() -> Result<HWND, ShellError> {
    if let Some(hwnd) = HOST.with(|h| (h.get() != 0).then(|| HWND(h.get() as *mut _))) {
        return Ok(hwnd);
    }
    // SAFETY: registers a window class (a second registration fails harmlessly) and creates a
    // message-only window owned by this thread.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).map_err(ShellError::Com)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: instance.into(),
            lpszClassName: w!("SwitchExplorerModalHost"),
            ..Default::default()
        };
        RegisterClassExW(&raw const class);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SwitchExplorerModalHost"),
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
        .map_err(ShellError::Com)?
    };
    HOST.with(|h| h.set(hwnd.0 as isize));
    Ok(hwnd)
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_RUN_MODAL {
        guard((), run_next);
        return LRESULT(0);
    }
    // SAFETY: default handling for every other message of our window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn run_next() {
    // A wake-up dispatched by a nested modal loop must not start a second job.
    if BUSY.with(Cell::get) {
        return;
    }
    let Some(job) = QUEUE.with(|queue| queue.borrow_mut().pop_front()) else {
        return;
    };
    BUSY.with(|busy| busy.set(true));
    // A panicking job drops its sender, so its caller sees `Canceled`; the host carries on.
    guard((), job);
    BUSY.with(|busy| busy.set(false));
    if QUEUE.with(|queue| !queue.borrow().is_empty()) {
        let _ = wake();
    }
}
