//! Runs blocking OS modal loops (DoDragDrop, TrackPopupMenuEx, shell dialogs) from our own hidden
//! message-only window instead of from inside a GPUI task.
//!
//! Why: a modal loop keeps pumping messages, so GPUI executes other tasks *inside* the task that
//! started the loop. With gpui-kit's `profiler` feature that nesting panics
//! (`ThreadTimings::save_task_timing` keeps one "running task" slot per thread), which is also what
//! gpui-kit's own `NativeMenu` would hit. Posting the job to this window runs it from the plain
//! message loop — outside GPUI's borrows *and* outside the executor — and the result comes back
//! through a oneshot channel that a foreground task awaits.

use std::cell::Cell;

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
}

/// Queues `job` to run on the UI thread from the host window's message handler.
/// If the job cannot be queued or panics, the receiver resolves to `Canceled`.
pub fn run<T: 'static>(job: impl FnOnce() -> T + 'static) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    let job: Job = Box::new(move || {
        let _ = tx.send(job());
    });
    let raw = Box::into_raw(Box::new(job));
    // SAFETY: on success the host window takes ownership of `raw` and frees it exactly once.
    let posted = host().and_then(|hwnd| unsafe {
        PostMessageW(Some(hwnd), WM_RUN_MODAL, WPARAM(0), LPARAM(raw as isize))
    });
    if let Err(err) = posted {
        log::error!(target: "sx_spike", "modal host: cannot queue job: {err}");
        // SAFETY: the message was not posted, so we still own `raw`.
        drop(unsafe { Box::from_raw(raw) });
    }
    rx
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
        // SAFETY: `lparam` is the pointer `run` handed over with this exact message.
        let job = unsafe { Box::from_raw(lparam.0 as *mut Job) };
        super::guard((), move || (*job)());
        return LRESULT(0);
    }
    // SAFETY: default handling for every other message.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
