//! Global hotkey (ADR-014): `RegisterHotKey` on a dedicated thread with its own message loop, so
//! WM_HOTKEY never depends on GPUI's loop or on nested modal loops (menus, drags).

use std::time::Instant;

use futures::channel::mpsc::UnboundedSender;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_D,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

pub const MASTER_DRAG: i32 = 1;
pub const MASTER_DRAG_COMBO: &str = "Ctrl+Shift+D";

#[derive(Debug)]
pub enum HotkeyEvent {
    Registered(&'static str),
    Failed { combo: &'static str, error: String },
    Fired { at: Instant },
}

pub fn spawn(tx: UnboundedSender<HotkeyEvent>) {
    let spawned = std::thread::Builder::new()
        .name("sx-hotkey".into())
        .spawn(move || {
            // SAFETY: thread-level registration (no HWND); released before the thread ends.
            let registered = unsafe {
                RegisterHotKey(
                    None,
                    MASTER_DRAG,
                    MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                    u32::from(VK_D.0),
                )
            };
            if let Err(err) = registered {
                let _ = tx.unbounded_send(HotkeyEvent::Failed {
                    combo: MASTER_DRAG_COMBO,
                    error: err.message(),
                });
                return;
            }
            let _ = tx.unbounded_send(HotkeyEvent::Registered(MASTER_DRAG_COMBO));
            let mut msg = MSG::default();
            // SAFETY: standard message loop for this thread's queue.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                if msg.message == WM_HOTKEY
                    && tx
                        .unbounded_send(HotkeyEvent::Fired { at: Instant::now() })
                        .is_err()
                {
                    break;
                }
            }
            // SAFETY: undoes the registration made above on the same thread.
            let _ = unsafe { UnregisterHotKey(None, MASTER_DRAG) };
        });
    if let Err(err) = spawned {
        log::error!(target: "sx_spike", "hotkey thread failed to start: {err}");
    }
}
