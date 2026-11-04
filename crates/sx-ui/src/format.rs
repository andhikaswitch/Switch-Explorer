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
        t!("status.items", count = count).into()
    }
}

/// `12 of 207 items` while the search box filters the folder.
pub fn filtered_count(shown: usize, total: usize) -> SharedString {
    if total == 1 {
        t!("status.filtered_one", shown = shown).into()
    } else {
        t!("status.filtered", shown = shown, total = total).into()
    }
}

pub fn selected_count(count: usize) -> SharedString {
    if count == 1 {
        t!("status.selected_one").into()
    } else {
        t!("status.selected", count = count).into()
    }
}
