//! File name rules (FEATURES.md F-OPS-03) and "Keep both" names.

use std::path::Path;

use thiserror::Error;

/// Windows' per-component limit (characters).
const MAX_NAME: usize = 255;
const INVALID: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("the name is empty")]
    Empty,
    #[error("the name contains {0:?}, which Windows does not allow")]
    InvalidChar(char),
    #[error("{0} is reserved by Windows")]
    Reserved(String),
    #[error("the name ends with a dot or a space")]
    TrailingDotOrSpace,
    #[error("the name is longer than 255 characters")]
    TooLong,
}

/// Checks a new name for a file or folder.
pub fn validate(name: &str) -> Result<(), NameError> {
    if name.trim().is_empty() {
        return Err(NameError::Empty);
    }
    if let Some(c) = name
        .chars()
        .find(|c| INVALID.contains(c) || u32::from(*c) < 32)
    {
        return Err(NameError::InvalidChar(c));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(NameError::TrailingDotOrSpace);
    }
    if name.encode_utf16().count() > MAX_NAME {
        return Err(NameError::TooLong);
    }
    // "CON", "con.txt" and "COM1.tar.gz" are all reserved: the part before the first dot counts.
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(NameError::Reserved(stem.to_uppercase()));
    }
    Ok(())
}

/// The extension Explorer shows (`"pdf"` for `a.pdf`), `None` for folders-like names and
/// dotfiles.
pub fn extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then_some(ext)
}

/// The part of a name selected when renaming starts: everything before the extension.
pub fn stem_len(name: &str) -> usize {
    extension(name).map_or(name.len(), |ext| name.len() - ext.len() - 1)
}

/// `name (2).ext`, `name (3).ext`, … — the first that `exists` says is free in `dir`.
pub fn keep_both(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> String {
    if !exists(&dir.join(name)) {
        return name.to_string();
    }
    let (stem, ext) = match extension(name) {
        Some(ext) => (&name[..name.len() - ext.len() - 1], Some(ext)),
        None => (name, None),
    };
    (2..=u32::MAX)
        .map(|n| match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        })
        .find(|candidate| !exists(&dir.join(candidate)))
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_what_windows_rejects() {
        assert_eq!(validate(""), Err(NameError::Empty));
        assert_eq!(validate("a:b"), Err(NameError::InvalidChar(':')));
        assert_eq!(validate("a\tb"), Err(NameError::InvalidChar('\t')));
        assert_eq!(validate("con"), Err(NameError::Reserved("CON".into())));
        assert_eq!(
            validate("Com1.tar.gz"),
            Err(NameError::Reserved("COM1".into()))
        );
        assert_eq!(validate("report."), Err(NameError::TrailingDotOrSpace));
        assert_eq!(validate(&"x".repeat(256)), Err(NameError::TooLong));
        assert!(validate("console.log").is_ok());
        assert!(validate("Laporan 2026 (final).xlsx").is_ok());
        assert!(validate(".gitignore").is_ok());
    }

    #[test]
    fn stems_and_extensions() {
        assert_eq!(stem_len("photo.final.jpg"), "photo.final".len());
        assert_eq!(stem_len(".gitignore"), ".gitignore".len());
        assert_eq!(stem_len("Makefile"), 8);
        assert_eq!(extension("a.PDF"), Some("PDF"));
    }

    #[test]
    fn keep_both_numbers_like_explorer() {
        let taken = ["a.txt", "a (2).txt", "dir"];
        let exists = |p: &Path| taken.iter().any(|t| p.ends_with(t));
        let dir = Path::new(r"C:\x");
        assert_eq!(keep_both(dir, "a.txt", exists), "a (3).txt");
        assert_eq!(keep_both(dir, "dir", exists), "dir (2)");
        assert_eq!(keep_both(dir, "b.txt", exists), "b.txt");
    }
}
