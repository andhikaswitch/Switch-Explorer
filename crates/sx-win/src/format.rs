//! Locale-aware formatting that matches Explorer's details view: dates in the user's short
//! date and time format, sizes in KB with the user's digit grouping (`StrFormatKBSizeW`).

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Globalization::{
    DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx};
use windows::Win32::UI::Shell::StrFormatKBSizeW;
use windows::core::PCWSTR;

/// Short date and time in the user's locale and time zone, e.g. `12/05/2025 21:10`.
/// `None` for zero or out-of-range times.
pub fn file_time(ticks: i64) -> Option<String> {
    if ticks <= 0 {
        return None;
    }
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: all pointers refer to locals; the dynamic time zone is the current one (None).
    unsafe {
        FileTimeToSystemTime(&raw const ft, &raw mut utc).ok()?;
        SystemTimeToTzSpecificLocalTimeEx(None, &raw const utc, &raw mut local).ok()?;
    }
    let mut buf = [0u16; 128];
    // SAFETY: null locale = user default; `local` is a valid SYSTEMTIME; the buffer length is
    // passed with the slice. The return value counts the terminating NUL.
    let date = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            DATE_SHORTDATE,
            Some(&raw const local),
            PCWSTR::null(),
            Some(&mut buf),
            PCWSTR::null(),
        )
    };
    if date <= 0 {
        return None;
    }
    let mut out = String::from_utf16_lossy(&buf[..date as usize - 1]);
    // SAFETY: as above.
    let time = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            Some(&raw const local),
            PCWSTR::null(),
            Some(&mut buf),
        )
    };
    if time > 0 {
        out.push(' ');
        out.push_str(&String::from_utf16_lossy(&buf[..time as usize - 1]));
    }
    Some(out)
}

/// Size in kilobytes as Explorer's Size column shows it: rounded up, grouped, e.g. `1,234 KB`.
pub fn kb_size(bytes: u64) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: the buffer length is passed with the slice; the function NUL-terminates it.
    let ok =
        !unsafe { StrFormatKBSizeW(i64::try_from(bytes).unwrap_or(i64::MAX), &mut buf) }.is_null();
    if !ok {
        return format!("{} KB", bytes.div_ceil(1024));
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_known_time() {
        // 2025-05-12 14:10:00 UTC.
        let ticks = 133_915_398_000_000_000;
        let text = file_time(ticks).unwrap();
        assert!(text.contains("2025") || text.contains("25"), "{text}");
        assert!(text.contains(' '), "{text}");
        assert_eq!(file_time(0), None);
    }

    #[test]
    fn sizes_round_up_to_kb() {
        let one = kb_size(1);
        assert!(one.starts_with('1') && one.ends_with("KB"), "{one}");
        assert!(kb_size(0).starts_with('0'));
        let big = kb_size(10 * 1024 * 1024);
        assert!(big.contains("240"), "{big}");
    }
}
