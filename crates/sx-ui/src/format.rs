//! Display strings for rows, drives and the status bar. Dates and sizes use the Windows locale
//! (FEATURES.md F-I18N-01), the same formats as Explorer.

use gpui_kit::SharedString;
use rust_i18n::t;
use sx_core::EntryKind;
use sx_fs::EntryView;
use sx_win::{Drive, DriveKind};

#[derive(Clone, Debug)]
pub struct RowText {
    pub name: SharedString,
    /// The part of `name` that matches the filter, to show in bold.
    pub matched: Option<std::ops::Range<usize>>,
    /// Search results: the folder the item was found in (relative to where the search ran),
    /// and its full path for the tooltip.
    pub folder: SharedString,
    pub folder_full: SharedString,
    pub modified: SharedString,
    pub kind: SharedString,
    pub size: SharedString,
}

/// The display strings of a row. Without `show_extensions` a file's extension is left out of
/// its name (the Type column still says it), as in Explorer.
pub fn row_text(entry: &EntryView<'_>, show_extensions: bool) -> RowText {
    let kind = match entry.kind {
        // Explorer also calls junctions and folder symlinks "File folder".
        EntryKind::Dir | EntryKind::Junction => t!("details.type_folder"),
        EntryKind::File | EntryKind::Symlink if entry.ext.is_empty() => t!("details.type_file"),
        EntryKind::File | EntryKind::Symlink => {
            t!("details.type_ext", ext = entry.ext.to_uppercase())
        }
    };
    let size = if entry.kind.is_container() {
        SharedString::default()
    } else {
        sx_win::format::kb_size(entry.size).into()
    };
    let mut name = entry.name;
    if !show_extensions && !entry.kind.is_container() && !entry.ext.is_empty() {
        // `ext` is the lowercase extension, the same length as in the name.
        let cut = name.len().saturating_sub(entry.ext.len() + 1);
        if cut > 0 && name.is_char_boundary(cut) && name[cut..].starts_with('.') {
            name = &name[..cut];
        }
    }
    RowText {
        name: SharedString::from(name.to_string()),
        matched: None,
        folder: SharedString::default(),
        folder_full: SharedString::default(),
        modified: sx_win::format::file_time(entry.modified)
            .unwrap_or_default()
            .into(),
        kind: kind.into(),
        size,
    }
}

/// `Local Disk (C:)` or `Data (D:)`, as Explorer names drives.
pub fn drive_label(drive: &Drive) -> SharedString {
    let name = if drive.label.is_empty() {
        match drive.kind {
            DriveKind::Fixed => t!("drive.local"),
            DriveKind::Removable => t!("drive.removable"),
            DriveKind::Network => t!("drive.network"),
            DriveKind::Optical => t!("drive.optical"),
            DriveKind::Ram => t!("drive.ram"),
            DriveKind::Unknown => t!("drive.other"),
        }
        .into_owned()
    } else {
        drive.label.clone()
    };
    format!("{name} ({}:)", drive.letter).into()
}

/// `212 GB free of 476 GB`, or "No media".
pub fn drive_space(drive: &Drive) -> SharedString {
    match drive.space {
        Some(space) => t!(
            "this_pc.free_of",
            free = sx_win::format::byte_size(space.free),
            total = sx_win::format::byte_size(space.total)
        )
        .into(),
        None => t!("this_pc.no_media").into(),
    }
}

/// `335 GB of 476 GB`, used of total, or "No media".
pub fn drive_used(drive: &Drive) -> SharedString {
    match drive.space {
        Some(space) => t!(
            "home.used_of",
            used = sx_win::format::byte_size(space.total.saturating_sub(space.free)),
            total = sx_win::format::byte_size(space.total)
        )
        .into(),
        None => t!("this_pc.no_media").into(),
    }
}

/// FILETIME ticks (100 ns since 1601) of a system time.
fn ticks_of(time: std::time::SystemTime) -> Option<i64> {
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_nanos() / 100)
        .ok()?
        .checked_add(UNIX_EPOCH_TICKS)
}

/// How long ago `then` was, as people say it: "Just now", "5 minutes ago", "Yesterday" (the
/// calendar day before today), "3 days ago"; from a week on, the date.
pub fn ago(then: std::time::SystemTime, now: std::time::SystemTime) -> SharedString {
    let day = |time| ticks_of(time).and_then(sx_win::format::local_day);
    let days = day(now).zip(day(then)).map(|(now, then)| now - then);
    let date = || ticks_of(then).and_then(sx_win::format::file_date);
    ago_text(
        now.duration_since(then).map_or(0, |d| d.as_secs()),
        days,
        date,
    )
}

/// [`ago`] from its parts: the seconds since then, the calendar days between the two (when
/// known), and the date to show from a week on.
fn ago_text(
    seconds: u64,
    days: Option<i64>,
    date: impl FnOnce() -> Option<String>,
) -> SharedString {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    // A time in the future (a clock that was set back) reads as now.
    match seconds {
        s if s < MINUTE => t!("time.just_now"),
        s if s < 2 * MINUTE => t!("time.minute_ago"),
        s if s < HOUR => t!("time.minutes_ago", count = s / MINUTE),
        s if s < 2 * HOUR => t!("time.hour_ago"),
        s if s < DAY => t!("time.hours_ago", count = s / HOUR),
        s => {
            // By the calendar: 25 hours ago can be yesterday or the day before.
            let days = days.unwrap_or((s / DAY) as i64).max(1);
            match days {
                1 => t!("time.yesterday"),
                2..=6 => t!("time.days_ago", count = days),
                _ => return date().unwrap_or_default().into(),
            }
        }
    }
    .into()
}

/// Why a folder could not be listed, as a sentence with a fix.
pub fn load_error(err: &sx_fs::LoadError) -> SharedString {
    use sx_fs::LoadError;
    // ERROR_NOT_READY; ERROR_BAD_NETPATH, ERROR_NETNAME_DELETED, ERROR_BAD_NET_NAME,
    // ERROR_NETWORK_UNREACHABLE.
    const NOT_READY: u32 = 21;
    const NETWORK: [u32; 4] = [53, 64, 67, 1231];
    match err {
        LoadError::NotFound(_) => t!("error.not_found"),
        LoadError::AccessDenied(_) => t!("error.access_denied"),
        LoadError::NotAFolder(_) => t!("error.not_a_folder"),
        LoadError::Other(source) => match source.win32_code() {
            Some(NOT_READY) => t!("error.not_ready"),
            Some(code) if NETWORK.contains(&code) => t!("error.network"),
            code => t!("error.other", code = code.unwrap_or(0)),
        },
        LoadError::Cancelled => t!("error.other", code = 0),
    }
    .into()
}

/// Why an item could not be opened, as a sentence with a fix.
pub fn open_error(name: &str, err: &sx_shell::ShellError) -> SharedString {
    // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SHARING_VIOLATION, ERROR_NO_ASSOCIATION.
    const MISSING: [u32; 2] = [2, 3];
    const IN_USE: u32 = 32;
    const NO_APP: u32 = 1155;
    match err.win32_code() {
        Some(NO_APP) => t!("error.open_no_app", name = name),
        Some(IN_USE) => t!("error.open_in_use", name = name),
        Some(code) if MISSING.contains(&code) => t!("error.open_missing", name = name),
        code => t!("error.open_failed", name = name, code = code.unwrap_or(0)),
    }
    .into()
}

pub fn item_count(count: usize) -> SharedString {
    if count == 1 {
        t!("status.items_one").into()
    } else {
        t!("status.items", count = number(count)).into()
    }
}

/// The Folder column of a search result: the folder from where the search ran, starting with
/// that folder's name (`Received\docs\2026`; just `Received` for items found directly in it).
pub fn found_in(root: Option<&std::path::Path>, folder: &std::path::Path) -> SharedString {
    let Some((root, rel)) = root.and_then(|root| Some((root, folder.strip_prefix(root).ok()?)))
    else {
        return folder.display().to_string().into();
    };
    let base = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    if rel.as_os_str().is_empty() {
        // `join("")` would add a trailing separator.
        return base.into();
    }
    std::path::Path::new(&base)
        .join(rel)
        .display()
        .to_string()
        .into()
}

/// A count with digit groups as the UI language writes them (`100,000`, `100.000`).
pub fn number(n: usize) -> String {
    let separator = if &*rust_i18n::locale() == "id" {
        '.'
    } else {
        ','
    };
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(digit);
    }
    out
}

/// The message for a search query with a bad operator value (cause and fix, DESIGN.md §10).
pub fn query_error(err: &sx_search::QueryError) -> SharedString {
    match err {
        sx_search::QueryError::Size(value) => t!("search.bad_size", value = value),
        sx_search::QueryError::Age(value) => t!("search.bad_age", value = value),
        sx_search::QueryError::Kind(value) => t!("search.bad_kind", value = value),
    }
    .into()
}

/// `120 results` found by a recursive search.
pub fn found_count(count: usize) -> SharedString {
    if count == 1 {
        t!("search.found_one").into()
    } else {
        t!("search.found", count = number(count)).into()
    }
}

/// `12 of 207 items` while the search box filters the folder.
pub fn filtered_count(shown: usize, total: usize) -> SharedString {
    if total == 1 {
        t!("status.filtered_one", shown = number(shown)).into()
    } else {
        t!(
            "status.filtered",
            shown = number(shown),
            total = number(total)
        )
        .into()
    }
}

pub fn selected_count(count: usize) -> SharedString {
    if count == 1 {
        t!("status.selected_one").into()
    } else {
        t!("status.selected", count = number(count)).into()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn found_in_starts_with_the_searched_folder() {
        let root = Path::new(r"C:\Users\Raka\Received");
        assert_eq!(found_in(Some(root), root).as_ref(), "Received");
        assert_eq!(
            found_in(Some(root), &root.join("docs").join("2026")).as_ref(),
            r"Received\docs\2026"
        );
        let other = Path::new(r"D:\Elsewhere");
        assert_eq!(found_in(Some(root), other).as_ref(), r"D:\Elsewhere");
    }

    #[test]
    fn times_ago_read_like_speech() {
        // The default locale is English.
        let text =
            |seconds: u64, days: Option<i64>| ago_text(seconds, days, || Some("12/05/2025".into()));
        assert_eq!(text(5, None).as_ref(), "Just now");
        assert_eq!(text(60, None).as_ref(), "1 minute ago");
        assert_eq!(text(59 * 60, None).as_ref(), "59 minutes ago");
        assert_eq!(text(3600, None).as_ref(), "1 hour ago");
        // Late last night is still "hours ago", whatever the calendar says.
        assert_eq!(text(23 * 3600, Some(1)).as_ref(), "23 hours ago");
        // 25 hours ago: yesterday, or the day before, by the calendar.
        assert_eq!(text(25 * 3600, Some(1)).as_ref(), "Yesterday");
        assert_eq!(text(25 * 3600, Some(2)).as_ref(), "2 days ago");
        assert_eq!(text(3 * 86_400, None).as_ref(), "3 days ago");
        assert_eq!(text(30 * 86_400, Some(30)).as_ref(), "12/05/2025");
    }

    #[test]
    fn times_ago_use_the_clock_and_the_calendar() {
        use std::time::{Duration, SystemTime};
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(500_000);
        assert_eq!(
            ago(now - Duration::from_secs(90), now).as_ref(),
            "1 minute ago"
        );
        // The clock went back: not "in the future".
        assert_eq!(ago(now + Duration::from_secs(90), now).as_ref(), "Just now");
        // From a week on: a date, without a time of day.
        let old = ago(now - Duration::from_hours(24 * 30), now);
        assert!(!old.is_empty() && !old.contains(':'), "{old}");
    }

    #[test]
    fn numbers_get_digit_groups() {
        // The default locale is English.
        assert_eq!(number(7), "7");
        assert_eq!(number(1234), "1,234");
        assert_eq!(number(100_000), "100,000");
        assert_eq!(number(1_234_567), "1,234,567");
    }
}
