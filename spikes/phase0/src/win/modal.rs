//! Runs blocking OS modal loops (DoDragDrop, TrackPopupMenuEx, shell dialogs) from our own hidden
//! message-only window instead of from inside a GPUI task (ADR-015).
//!
//! Why: a modal loop keeps pumping messages, so GPUI executes other tasks *inside* the task that
//! started the loop. With gpui-kit's `profiler` feature that nesting panics
//! (`ThreadTimings::save_task_timing` keeps one "running task" slot per thread), which is also what
//! gpui-kit's own `NativeMenu` would hit. Posting a wake-up to this window runs the job from the
//! plain message loop — outside GPUI's borrows *and* outside the executor — and the result comes
//! back through a oneshot channel that a foreground task awaits.
//!
//! Jobs wait in a thread-local queue (never as pointers inside messages), and only one job runs at
//! a time: a nested loop that dispatches another wake-up leaves the queue alone, and the outer job
//! re-posts when it finishes.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use futures::channel::oneshot;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_MESSAGE, PostMessageW, RegisterClassExW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WNDCLASSEXW,
};
use windows::core::{Result, w};

const WM_RUN_MODAL: u32 = WM_APP + 0x51;

type Job = Box<dyn FnOnce()>;

thread_local! {
    static HOST: Cell<isize> = const { Cell::new(0) };
    static QUEUE: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// Creates the host window. Call once on the UI thread at startup, so it never ends up on
/// another thread by accident.
pub fn init() -> Result<()> {
    host().map(|_| ())
}

/// Queues `job` to run on the UI thread from the host window's message handler. If the wake-up
/// cannot be posted or the job panics, the receiver resolves to `Canceled`.
pub fn run<T: 'static>(job: impl FnOnce() -> T + 'static) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    QUEUE.with(|queue| {
        queue.borrow_mut().push_back(Box::new(move || {
            let _ = tx.send(job());
        }));
    });
    if let Err(err) = wake() {
        log::error!(target: "sx_spike", "modal host: cannot queue job: {err}");
        // Dropping the queued job drops its sender, so the awaiting task sees `Canceled`.
        QUEUE.with(|queue| queue.borrow_mut().pop_back());
    }
    rx
}

fn wake() -> Result<()> {
    let hwnd = host()?;
    // SAFETY: posts a data-less message to our own window.
    unsafe { PostMessageW(Some(hwnd), WM_RUN_MODAL, WPARAM(0), LPARAM(0)) }
}

fn host() -> Result<HWND> {
    if let Some(hwnd) = HOST.with(|h| (h.get() != 0).then(|| HWND(h.get() as *mut _))) {
        return Ok(hwnd);
    }
    // SAFETY: registers a window class and creates a message-only window on this thread.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: instance.into(),
            lpszClassName: w!("SxSpikeModalHost"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SxSpikeModalHost"),
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
        )?
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
        // A wake-up dispatched by a nested modal loop must not start a second job.
        if BUSY.with(Cell::get) {
            return LRESULT(0);
        }
        let Some(job) = QUEUE.with(|queue| queue.borrow_mut().pop_front()) else {
            return LRESULT(0);
        };
        BUSY.with(|busy| busy.set(true));
        super::guard((), job);
        BUSY.with(|busy| busy.set(false));
        if QUEUE.with(|queue| !queue.borrow().is_empty()) {
            let _ = wake();
        }
        return LRESULT(0);
    }
    // SAFETY: default handling for every other message.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
