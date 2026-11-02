//! System file icons (FEATURES.md "Ikon sistem"): the same icons as File Explorer, rendered to
//! straight-alpha BGRA bitmaps for the UI. Blocking and COM-dependent (icon handlers): call from
//! an STA worker, never the UI thread.

use std::path::Path;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON};
use windows::core::PCWSTR;

/// A rendered icon: `width × height` pixels, BGRA, straight (not premultiplied) alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconBitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// What an icon depends on. Most files share their type's icon; programs, shortcuts and icon
/// files carry their own.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    /// Lowercase extension without the dot ("" for none).
    Type(String),
    Folder,
    /// An item with an icon of its own (`.exe`, `.lnk`, `.ico`, …).
    Item(std::path::PathBuf),
}

/// Extensions whose files have their own icon.
const OWN_ICON: &[&str] = &[
    "exe",
    "lnk",
    "ico",
    "cur",
    "ani",
    "msi",
    "url",
    "scr",
    "appref-ms",
];

impl IconKey {
    pub fn for_item(path: &Path, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if OWN_ICON.contains(&ext.as_str()) {
            Self::Item(path.to_path_buf())
        } else {
            Self::Type(ext)
        }
    }
}

/// Owns an HICON and destroys it on drop.
struct Icon(HICON);

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: the icon came from SHGetFileInfoW (SHGFI_ICON), which hands ownership to us.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

fn shell_icon(key: &IconKey) -> Option<Icon> {
    let (query, attrs, flags): (String, FILE_FLAGS_AND_ATTRIBUTES, SHGFI_FLAGS) = match key {
        // By type: the file does not have to exist.
        IconKey::Type(ext) if ext.is_empty() => (
            "file".into(),
            FILE_ATTRIBUTE_NORMAL,
            SHGFI_USEFILEATTRIBUTES,
        ),
        IconKey::Type(ext) => (
            format!("file.{ext}"),
            FILE_ATTRIBUTE_NORMAL,
            SHGFI_USEFILEATTRIBUTES,
        ),
        IconKey::Folder => (
            "folder".into(),
            FILE_ATTRIBUTE_DIRECTORY,
            SHGFI_USEFILEATTRIBUTES,
        ),
        IconKey::Item(path) => (
            path.to_string_lossy().into_owned(),
            FILE_ATTRIBUTE_NORMAL,
            SHGFI_FLAGS(0),
        ),
    };
    let wide: Vec<u16> = query.encode_utf16().chain(std::iter::once(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return None;
    }
    let mut info = SHFILEINFOW::default();
    // SAFETY: `wide` is NUL-terminated; `info` is a local whose size is passed.
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            attrs,
            Some(&raw mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | flags,
        )
    };
    (ok != 0 && !info.hIcon.is_invalid()).then_some(Icon(info.hIcon))
}

/// Draws an icon into a `size × size` 32-bit bitmap and returns its pixels.
fn render(icon: &Icon, size: i32) -> Option<IconBitmap> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let len = (size * size * 4) as usize;
    // SAFETY: a memory DC and a DIB section we own, selected, drawn into, read and released in
    // order; the bits pointer is valid while the DIB exists and covers `len` bytes.
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(
            Some(dc),
            &raw const info,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        ) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, HGDIOBJ(bitmap.0));
        let drawn = DrawIconEx(dc, 0, 0, icon.0, size, size, 0, None, DI_NORMAL).is_ok();
        let pixels = (drawn && !bits.is_null())
            .then(|| std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec());
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        let mut bgra = pixels?;
        straighten(&mut bgra);
        Some(IconBitmap {
            width: size as u32,
            height: size as u32,
            bgra,
        })
    }
}

/// GDI leaves premultiplied alpha; old icons without an alpha channel leave alpha at 0.
fn straighten(bgra: &mut [u8]) {
    let has_alpha = bgra.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    for p in bgra.as_chunks_mut::<4>().0 {
        if !has_alpha {
            p[3] = if p[0] | p[1] | p[2] == 0 { 0 } else { 255 };
            continue;
        }
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// The icon for `key` at `size` pixels (32 is the shell's large icon).
pub fn load(key: &IconKey, size: u32) -> Option<IconBitmap> {
    let icon = shell_icon(key)?;
    render(&icon, i32::try_from(size).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StaGuard;

    #[test]
    fn folder_and_type_icons_render() {
        let _com = StaGuard::new().unwrap();
        for key in [
            IconKey::Folder,
            IconKey::Type("txt".into()),
            IconKey::Type(String::new()),
        ] {
            let icon = load(&key, 32).unwrap();
            assert_eq!(
                (icon.width, icon.height, icon.bgra.len()),
                (32, 32, 32 * 32 * 4)
            );
            assert!(
                icon.bgra.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{key:?} is empty"
            );
        }
    }

    #[test]
    fn keys() {
        assert_eq!(
            IconKey::for_item(Path::new(r"C:\a\B.PDF"), false),
            IconKey::Type("pdf".into())
        );
        assert!(matches!(
            IconKey::for_item(Path::new(r"C:\a\app.exe"), false),
            IconKey::Item(_)
        ));
        assert_eq!(IconKey::for_item(Path::new(r"C:\a"), true), IconKey::Folder);
    }

    #[test]
    fn premultiplied_alpha_is_undone() {
        let mut px = vec![64, 64, 64, 128, 0, 0, 0, 0];
        straighten(&mut px);
        assert_eq!(&px[..4], &[128, 128, 128, 128]);
    }
}
