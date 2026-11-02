//! Natural ("logical") order like Explorer. Windows sorts names linguistically (accents,
//! locale rules) with digit runs compared by value, so we take sort keys from the OS instead of
//! re-implementing collation: `LCMapStringEx(LCMAP_SORTKEY | NORM_IGNORECASE |
//! SORT_DIGITSASNUMBERS)` keys compare with plain byte order. `StrCmpLogicalW` is the reference
//! the keys are tested against.
//!
//! Known divergence: names with superscript digits or vulgar fractions (`¹ ² ³ ½`) can order
//! differently from `StrCmpLogicalW`, which treats them as digits; the keys do not. Every other
//! script and digit system tested (ASCII, accented Latin, ß/İ/ı, CJK, emoji, full-width,
//! Arabic-Indic and Devanagari digits) matches.

use std::cmp::Ordering;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Globalization::{
    LCMAP_SORTKEY, LCMapStringEx, NORM_IGNORECASE, SORT_DIGITSASNUMBERS,
};
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

const FLAGS: u32 = LCMAP_SORTKEY | NORM_IGNORECASE.0 | SORT_DIGITSASNUMBERS.0;
/// Initial key buffer in u16 units (2 KiB): enough for names of a few hundred characters.
const KEY_BUFFER: usize = 1024;

/// Builds natural sort keys with reused buffers: one `LCMapStringEx` call per name in the
/// common case (a second, sized call only for very long names).
#[derive(Debug, Default)]
pub struct SortKeys {
    wide: Vec<u16>,
    buffer: Vec<u16>,
}

impl SortKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the key of `name` to `out` and returns its byte range. Keys from the same user
    /// locale compare with `<[u8]>::cmp`. If Windows cannot produce a key, a lowercase UTF-16
    /// key is used instead, so the order degrades to case-insensitive code-unit order rather
    /// than enumeration order.
    pub fn append(&mut self, name: &str, out: &mut Vec<u8>) -> std::ops::Range<usize> {
        let start = out.len();
        self.wide.clear();
        self.wide.extend(name.encode_utf16());
        if self.wide.is_empty() {
            return start..start;
        }
        if self.buffer.len() < KEY_BUFFER {
            self.buffer.resize(KEY_BUFFER, 0);
        }
        let mut written = self.map(true);
        if written == 0 {
            // Too small (ERROR_INSUFFICIENT_BUFFER) or failed: ask for the size and retry once.
            let needed = self.map(false);
            if needed > 0 {
                self.buffer.resize(needed, 0);
                written = self.map(true);
            }
        }
        if written == 0 {
            out.extend(
                name.to_lowercase()
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes),
            );
        } else {
            out.extend(
                self.buffer
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .take(written),
            );
        }
        start..out.len()
    }

    /// One `LCMapStringEx` call: with `dest`, writes into `buffer` and returns the bytes written;
    /// without, returns the bytes needed. 0 on failure.
    fn map(&mut self, dest: bool) -> usize {
        // The wrapper passes the slice length as `cchDest`, which counts *bytes* for sort keys,
        // while the u16 buffer holds twice that: the API never writes past the buffer.
        let target = dest.then_some(&mut self.buffer[..]);
        // SAFETY: `wide` is a valid UTF-16 slice; the destination (if any) is a live, owned
        // buffer of at least `cchDest` bytes; null locale = user default.
        let result = unsafe {
            LCMapStringEx(
                PCWSTR::null(),
                FLAGS,
                &self.wide,
                target,
                None,
                None,
                LPARAM(0),
            )
        };
        let bytes = usize::try_from(result).unwrap_or(0);
        if dest {
            bytes.min(self.buffer.len())
        } else {
            bytes
        }
    }
}

/// Appends the natural sort key of `name` to `out` (see [`SortKeys::append`]). Prefer a reused
/// [`SortKeys`] when keying many names.
pub fn natural_sort_key(name: &str, out: &mut Vec<u8>) -> std::ops::Range<usize> {
    SortKeys::new().append(name, out)
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
    fn long_names_take_the_retry_path() {
        let long_a = format!("{}2", "x".repeat(2000));
        let long_b = format!("{}10", "x".repeat(2000));
        let mut keys = SortKeys::new();
        let mut arena = Vec::new();
        let a = keys.append(&long_a, &mut arena);
        let b = keys.append(&long_b, &mut arena);
        assert!(
            a.len() > KEY_BUFFER,
            "the key does not fit the first buffer"
        );
        assert!(arena[a] < arena[b]);
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
