//! Windows-only helpers. Every callback Windows makes into this code is wrapped in
//! `catch_unwind` so a panic never crosses the FFI boundary.

pub mod dnd;
pub mod hotkey;
pub mod menu;
pub mod modal;
pub mod theme_watch;

use std::time::Duration;

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{FILETIME, HWND};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::SystemInformation::{GetSystemTimePreciseAsFileTime, OSVERSIONINFOW};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// A window handle copied as an integer so it can move into foreground tasks. It is only
/// turned back into an `HWND` on the UI thread while the window is alive.
#[derive(Clone, Copy, Debug)]
pub struct Hwnd(isize);

impl Hwnd {
    pub fn get(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }
}

pub fn hwnd(window: &Window) -> Option<Hwnd> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(Hwnd(handle.hwnd.get())),
        _ => None,
    }
}

/// Windows build number (22621 = Windows 11 22H2, the first build with system backdrops).
pub fn os_build() -> u32 {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: RtlGetVersion only writes into the struct we own; its size field is set.
    let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut info) };
    if status.is_ok() {
        info.dwBuildNumber
    } else {
        0
    }
}

/// Private working set of this process — the same figure as Task Manager's
/// "Memory (active private working set)" column used by PERFORMANCE.md.
pub fn private_working_set() -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: the EX2 struct starts with PROCESS_MEMORY_COUNTERS and `cb` declares its full size.
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .ok()?;
    Some(counters.PrivateWorkingSetSize as u64)
}

/// Time since the OS created this process (covers loader time before `main`).
pub fn process_age() -> Option<Duration> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: all four out-pointers refer to locals; the pseudo-handle needs no closing.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    // SAFETY: plain query without arguments.
    let now = unsafe { GetSystemTimePreciseAsFileTime() };
    let ticks = |ft: FILETIME| (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
    Some(Duration::from_nanos(
        ticks(now).saturating_sub(ticks(created)) * 100,
    ))
}

/// Name of DXGI adapter 0. GPUI uses the first adapter that can create a D3D11 device, which is
/// adapter 0 on normal systems; the per-app GPU preference in Windows Settings reorders it.
pub fn primary_adapter() -> String {
    // SAFETY: DXGI factory/adapter queries with locally owned COM pointers.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return String::from("unknown");
        };
        let Ok(adapter) = factory.EnumAdapters1(0) else {
            return String::from("unknown");
        };
        let Ok(desc) = adapter.GetDesc1() else {
            return String::from("unknown");
        };
        let len = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        String::from_utf16_lossy(&desc.Description[..len])
    }
}

/// Light/dark frame and backdrop tint (GPUI only sets this when its own appearance callback runs).
pub fn set_dark_frame(hwnd: Hwnd, dark: bool) {
    use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let value = windows::core::BOOL::from(dark);
    // SAFETY: DWM reads a BOOL from a local whose size is passed alongside.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd.get(),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const value).cast(),
            size_of::<windows::core::BOOL>() as u32,
        )
    };
}

/// Runs a Windows callback body, turning a panic into `fallback` instead of unwinding into the OS.
pub fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        log::error!(target: "sx_spike", "panic caught at an FFI boundary");
        fallback
    })
}
