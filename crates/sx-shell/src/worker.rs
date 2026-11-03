//! A long-lived STA worker that pumps messages while it waits (ARCHITECTURE.md §5). Shell
//! objects made on it (icon handlers, thumbnail providers, context menus) may post messages to
//! their thread or be called from other apartments, and an STA only delivers those while it
//! pumps. A worker that just blocked on a channel would leave them hanging.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage,
};
use windows::core::PCWSTR;

use crate::com::StaGuard;
use crate::error::ShellError;

/// Which queued job runs next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// First come, first served.
    Oldest,
    /// The latest request first: for work that follows what is on screen (icons while
    /// scrolling), where the newest request is the one the user is looking at.
    Newest,
}

type Job = Box<dyn FnOnce() + Send>;

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    /// Auto-reset: set when a job is queued or the worker should stop.
    wake: HANDLE,
    stopped: AtomicBool,
    order: Order,
}

// SAFETY: the event handle may be signalled from any thread; it is closed once, by Drop, after
// the last owner (the worker or its thread) is gone.
unsafe impl Send for Queue {}
// SAFETY: as above; the jobs are behind a mutex.
unsafe impl Sync for Queue {}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: our own event, closed once.
        let _ = unsafe { CloseHandle(self.wake) };
    }
}

impl Queue {
    fn wake(&self) {
        // SAFETY: signals our own event.
        let _ = unsafe { SetEvent(self.wake) };
    }

    fn next(&self) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        match self.order {
            Order::Oldest => jobs.pop_front(),
            Order::Newest => jobs.pop_back(),
        }
    }
}

/// Runs jobs on its own STA thread until dropped (queued jobs that have not started are
/// dropped with it).
pub struct StaWorker {
    queue: Arc<Queue>,
}

impl std::fmt::Debug for StaWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaWorker")
            .field("order", &self.queue.order)
            .finish_non_exhaustive()
    }
}

impl Drop for StaWorker {
    fn drop(&mut self) {
        self.queue.stopped.store(true, Ordering::Relaxed);
        self.queue.wake();
    }
}

/// Dispatches every message waiting for this thread.
fn pump() {
    let mut msg = MSG::default();
    // SAFETY: the standard message loop on this thread's own queue; `msg` is a local.
    unsafe {
        while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
}

impl StaWorker {
    pub fn spawn(name: &str, order: Order) -> Result<Self, ShellError> {
        // SAFETY: an unnamed auto-reset event, owned by the queue.
        let wake =
            unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(ShellError::Com)?;
        let queue = Arc::new(Queue {
            jobs: Mutex::default(),
            wake,
            stopped: AtomicBool::new(false),
            order,
        });
        let worker = queue.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _com = match StaGuard::new() {
                    Ok(com) => com,
                    Err(err) => {
                        tracing::warn!(error = %err, "STA worker cannot start COM");
                        worker.stopped.store(true, Ordering::Relaxed);
                        return;
                    }
                };
                loop {
                    pump();
                    if worker.stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(job) = worker.next() {
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                            tracing::error!("panic in an STA worker job");
                        }
                        continue;
                    }
                    // Nothing to do: sleep until a job, a stop, or a message arrives.
                    // SAFETY: waits on our own event while letting messages wake the thread.
                    let _ = unsafe {
                        MsgWaitForMultipleObjectsEx(
                            Some(&[worker.wake]),
                            INFINITE,
                            QS_ALLINPUT,
                            MWMO_INPUTAVAILABLE,
                        )
                    };
                }
                // Jobs that never ran may hold COM objects of this apartment: release them
                // while it still exists.
                worker
                    .jobs
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
            })
            .map_err(ShellError::Thread)?;
        Ok(Self { queue })
    }

    /// Queues `job`. `false` when the worker has stopped (its thread could not start COM).
    pub fn run(&self, job: impl FnOnce() + Send + 'static) -> bool {
        if self.queue.stopped.load(Ordering::Relaxed) {
            return false;
        }
        self.queue
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Box::new(job));
        self.queue.wake();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn runs_jobs_in_an_sta_in_the_chosen_order() {
        let worker = StaWorker::spawn("sx-test-sta", Order::Newest).unwrap();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let (tx, rx) = mpsc::channel();
        // Hold the worker so the next three queue up behind it.
        assert!(worker.run(move || {
            let _ = gate_rx.recv_timeout(Duration::from_secs(5));
        }));
        for n in 1..=3 {
            let tx = tx.clone();
            assert!(worker.run(move || {
                // The thread is already in an STA, so entering one again is fine (S_FALSE).
                let in_sta = StaGuard::new().is_ok();
                let _ = tx.send((n, in_sta));
            }));
        }
        gate_tx.send(()).unwrap();
        let got: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(got, vec![(3, true), (2, true), (1, true)]);
    }

    #[test]
    fn a_panicking_job_does_not_stop_the_worker() {
        let worker = StaWorker::spawn("sx-test-sta", Order::Oldest).unwrap();
        let (tx, rx) = mpsc::channel();
        assert!(worker.run(|| panic!("boom")));
        assert!(worker.run(move || {
            let _ = tx.send(());
        }));
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
