//! Address bar input (FEATURES.md F-NAV-03): plain and relative paths, forward slashes, quotes,
//! `%VAR%` expansion and `shell:` names. Purely lexical; whether the folder exists is decided
//! when it loads.

use std::path::{Component, Path, PathBuf};

use sx_core::Location;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Location(Location),
    /// `shell:Downloads` and friends, resolved by the shell (`SHParseDisplayName`).
    Shell(String),
}

/// Parses what the user typed. `current` resolves relative paths; without it they are rejected.
pub fn parse(input: &str, current: Option<&Path>) -> Option<Address> {
    let text = input.trim().trim_matches('"').trim();
    if text.is_empty() {
        return None;
    }
    if let Some(name) = strip_prefix_ignore_case(text, "shell:") {
        return Some(Address::Shell(name.trim().to_string()));
    }
    let expanded = expand_home(&expand_vars(text)).replace('/', "\\");
    let mut path = PathBuf::from(&expanded);
    // `C:` alone means the drive root here, not "the current folder on C:".
    if expanded.len() == 2 && expanded.ends_with(':') {
        path = PathBuf::from(format!("{expanded}\\"));
    }
    if !path.is_absolute() {
        path = current?.join(path);
    }
    Some(Address::Location(Location::Path(normalize(&path))))
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `~` and `~\rest` mean the user profile folder (`%USERPROFILE%`).
fn expand_home(text: &str) -> String {
    let rest = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['\\', '/']) => rest,
        _ => return text.to_string(),
    };
    match std::env::var("USERPROFILE") {
        Ok(home) => format!("{home}{rest}"),
        Err(_) => text.to_string(),
    }
}

/// Replaces `%NAME%` with the environment variable; unknown names stay as typed (like cmd.exe).
pub fn expand_vars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                if let Ok(value) = std::env::var(name) {
                    out.push_str(&value);
                } else {
                    out.push('%');
                    out.push_str(name);
                    out.push('%');
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolves `.` and `..` lexically; `..` never climbs above the root.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.parent().is_some() && out.file_name().is_some() {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps)]
    fn loc(s: &str) -> Option<Address> {
        Some(Address::Location(Location::path(s)))
    }

    #[test]
    fn absolute_paths_and_slashes() {
        assert_eq!(parse(r"C:\Users", None), loc(r"C:\Users"));
        assert_eq!(parse("C:/Users/Raka/", None), loc(r"C:\Users\Raka"));
        assert_eq!(parse("  \"D:\\Data\"  ", None), loc(r"D:\Data"));
        assert_eq!(parse("d:", None), loc(r"d:\"));
        assert_eq!(parse(r"\\server\share\x", None), loc(r"\\server\share\x"));
    }

    #[test]
    fn relative_paths_need_a_current_folder() {
        let here = Path::new(r"C:\Users\Raka");
        assert_eq!(
            parse("Documents", Some(here)),
            loc(r"C:\Users\Raka\Documents")
        );
        assert_eq!(parse(r"..\Public", Some(here)), loc(r"C:\Users\Public"));
        assert_eq!(parse(r"..\..\..\..", Some(here)), loc(r"C:\"));
        assert_eq!(parse("Documents", None), None);
        assert_eq!(parse("   ", Some(here)), None);
    }

    #[test]
    fn variables_and_shell_names() {
        let windir = std::env::var("WINDIR").unwrap();
        assert_eq!(
            parse(r"%windir%\System32", None),
            loc(&format!(r"{windir}\System32"))
        );
        assert_eq!(expand_vars("%SX_NOT_SET_42%\\x"), "%SX_NOT_SET_42%\\x");
        assert_eq!(expand_vars("100% done"), "100% done");
        let home = std::env::var("USERPROFILE").unwrap();
        assert_eq!(parse("~", None), loc(&home));
        assert_eq!(
            parse(r"~\Documents", None),
            loc(&format!(r"{home}\Documents"))
        );
        assert_eq!(parse("~x", Some(Path::new(r"C:\a"))), loc(r"C:\a\~x"));
        assert_eq!(
            parse("Shell:Downloads", None),
            Some(Address::Shell("Downloads".into()))
        );
    }
}
