//! Operating system facts that decide fallbacks (SYSTEM-INTEGRATION.md §3).

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Globalization::GetUserDefaultUILanguage;
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

/// The Windows display language as a short code for the UI: `id` for Indonesian, `en` for
/// everything else (FEATURES.md F-I18N-01).
pub fn ui_language() -> &'static str {
    // LANG_INDONESIAN; the primary language id is the low 10 bits of the LANGID.
    const LANG_INDONESIAN: u16 = 0x21;
    // SAFETY: plain query without arguments.
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid & 0x3FF == LANG_INDONESIAN {
        "id"
    } else {
        "en"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_supported_build() {
        // Windows 10 22H2 is 19045; anything older is outside the support matrix.
        assert!(os_build() >= 19_045);
        assert!(matches!(ui_language(), "id" | "en"));
    }
}
