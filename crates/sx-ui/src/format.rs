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
    pub modified: SharedString,
    pub kind: SharedString,
    pub size: SharedString,
}

pub fn row_text(entry: &EntryView<'_>) -> RowText {
    let kind = match entry.kind {
        EntryKind::Dir => t!("details.type_folder"),
        EntryKind::Junction => t!("details.type_link"),
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
    RowText {
        name: SharedString::from(entry.name.to_string()),
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

pub fn item_count(count: usize) -> SharedString {
    if count == 1 {
        t!("status.items_one").into()
    } else {
        t!("status.items", count = count).into()
    }
}

pub fn selected_count(count: usize) -> SharedString {
    if count == 1 {
        t!("status.selected_one").into()
    } else {
        t!("status.selected", count = count).into()
    }
}
