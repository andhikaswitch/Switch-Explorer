//! Natural ("logical") order exactly like Explorer. Windows sorts names linguistically (accents,
//! locale rules) with digit runs compared by value, so we take sort keys from the OS instead of
//! re-implementing collation: `LCMapStringEx(LCMAP_SORTKEY | NORM_IGNORECASE |
//! SORT_DIGITSASNUMBERS)` keys compare with plain byte order. `StrCmpLogicalW` is the reference
//! the keys are tested against.

use std::cmp::Ordering;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Globalization::{
    LCMAP_SORTKEY, LCMapStringEx, NORM_IGNORECASE, SORT_DIGITSASNUMBERS,
};
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

const FLAGS: u32 = LCMAP_SORTKEY | NORM_IGNORECASE.0 | SORT_DIGITSASNUMBERS.0;

/// Appends the natural sort key of `name` to `out` and returns its byte range. Keys from the
/// same user locale compare with `<[u8]>::cmp`.
pub fn natural_sort_key(name: &str, out: &mut Vec<u8>) -> std::ops::Range<usize> {
    let start = out.len();
    let wide: Vec<u16> = name.encode_utf16().collect();
    if wide.is_empty() {
        return start..start;
    }
    // SAFETY: a size query (no destination) on a valid UTF-16 slice; the user-default locale.
    let needed =
        unsafe { LCMapStringEx(PCWSTR::null(), FLAGS, &wide, None, None, None, LPARAM(0)) };
    let Ok(needed) = usize::try_from(needed) else {
        return start..start;
    };
    if needed == 0 {
        return start..start;
    }
    // For sort keys the API counts bytes; a u16 buffer of `needed` elements holds 2×needed bytes,
    // and the API is told `needed`, so it never writes past the buffer.
    let mut buffer = vec![0_u16; needed];
    // SAFETY: as above, with a destination of `needed` bytes inside a 2×needed-byte buffer.
    let written = unsafe {
        LCMapStringEx(
            PCWSTR::null(),
            FLAGS,
            &wide,
            Some(&mut buffer),
            None,
            None,
            LPARAM(0),
        )
    };
    let written = usize::try_from(written).unwrap_or(0).min(needed);
    out.extend(buffer.iter().flat_map(|w| w.to_le_bytes()).take(written));
    start..out.len()
}

/// `StrCmpLogicalW`, the comparison Explorer's own sort uses.
pub fn str_cmp_logical(a: &str, b: &str) -> Ordering {
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (a, b) = (wide(a), wide(b));
    // SAFETY: both buffers are NUL-terminated UTF-16 and live for the duration of the call.
    let result = unsafe { StrCmpLogicalW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr())) };
    result.cmp(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        natural_sort_key(name, &mut out);
        out
    }

    #[test]
    fn explorer_order() {
        assert_eq!(str_cmp_logical("file2", "file10"), Ordering::Less);
        assert_eq!(str_cmp_logical("File1", "file1"), Ordering::Equal);
        assert!(key("file2") < key("file10"));
        assert_eq!(key("File1"), key("file1"));
        assert!(
            key("émile") < key("file1"),
            "accents sort with their base letter"
        );
    }

    #[test]
    fn keys_append_to_an_arena() {
        let mut arena = Vec::new();
        let a = natural_sort_key("alpha", &mut arena);
        let b = natural_sort_key("beta", &mut arena);
        assert_eq!(a.end, b.start);
        assert!(arena[a] < arena[b]);
        assert_eq!(natural_sort_key("", &mut arena).len(), 0);
    }
}
