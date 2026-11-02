//! Operating system facts that decide fallbacks (SYSTEM-INTEGRATION.md §3).

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

/// First build with system backdrops (Windows 11 22H2). GPUI silently skips Mica below it.
pub const BUILD_SYSTEM_BACKDROP: u32 = 22_621;

/// The Windows build number, or 0 if it cannot be read. `RtlGetVersion` is used because
/// `GetVersionEx` lies to processes without a matching manifest entry.
pub fn os_build() -> u32 {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: RtlGetVersion only writes into the struct we own; its size field is set.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    if status.is_ok() {
        info.dwBuildNumber
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_supported_build() {
        // Windows 10 22H2 is 19045; anything older is outside the support matrix.
        assert!(os_build() >= 19_045);
    }
}
