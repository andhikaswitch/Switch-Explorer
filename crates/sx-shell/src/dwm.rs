//! DWM window attributes GPUI does not set when the theme changes outside its own callback
//! (SYSTEM-INTEGRATION.md §3).

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::core::BOOL;

/// Dark or light title bar frame and backdrop tint. `hwnd` is the raw window handle; an invalid
/// one makes DWM return an error, which is logged and ignored (the frame keeps its colour).
pub fn set_dark_frame(hwnd: isize, dark: bool) {
    let value = BOOL::from(dark);
    // SAFETY: DWM reads a BOOL from a local whose size is passed alongside; the handle is only
    // used as an identifier, which DWM validates.
    let result = unsafe {
        DwmSetWindowAttribute(
            HWND(hwnd as *mut core::ffi::c_void),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const value).cast(),
            size_of::<BOOL>() as u32,
        )
    };
    if let Err(err) = result {
        tracing::debug!(error = %err, "DWMWA_USE_IMMERSIVE_DARK_MODE rejected");
    }
}
