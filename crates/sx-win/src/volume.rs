//! Drive letters with their kind, label and capacity (sidebar "This PC" and the status bar).
//!
//! Every call here can block: an unreachable network drive or a slow optical drive takes seconds
//! to answer. Call it from a background executor, never on the UI thread.

use std::path::PathBuf;

use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetThreadErrorMode, THREAD_ERROR_MODE,
};
use windows::core::PCWSTR;

use crate::path::to_wide;

/// `GetDriveTypeW` results.
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;
const DRIVE_NO_ROOT_DIR: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Ram,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// `C:\`
    pub root: PathBuf,
    pub letter: char,
    pub kind: DriveKind,
    /// Volume label; empty when the volume has none or no media is inserted.
    pub label: String,
    /// Capacity in bytes; `None` when the drive has no media or cannot be queried.
    pub space: Option<Space>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub total: u64,
    /// Free bytes available to this user (quotas included), as Explorer shows.
    pub free: u64,
}

impl Space {
    /// Used fraction in 0.0..=1.0.
    pub fn used_fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.total.saturating_sub(self.free) as f64 / self.total as f64) as f32
        }
    }
}

/// Suppresses the "There is no disk in the drive" dialog for the calling thread while alive.
struct NoErrorDialogs(THREAD_ERROR_MODE);

impl NoErrorDialogs {
    fn new() -> Self {
        let mut old = THREAD_ERROR_MODE(0);
        // SAFETY: sets this thread's error mode; `old` is a local out-parameter.
        let _ = unsafe {
            SetThreadErrorMode(
                SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX,
                Some(&raw mut old),
            )
        };
        Self(old)
    }
}

impl Drop for NoErrorDialogs {
    fn drop(&mut self) {
        // SAFETY: restores the mode saved in `new` on the same thread.
        let _ = unsafe { SetThreadErrorMode(self.0, None) };
    }
}

/// All mounted drive letters, A to Z. Blocking (see the module docs).
pub fn drives() -> Vec<Drive> {
    let _guard = NoErrorDialogs::new();
    // SAFETY: plain query without arguments.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| drive(char::from(b'A' + bit)))
        .collect()
}

/// One drive letter, or `None` when it is not mounted.
pub fn drive(letter: char) -> Option<Drive> {
    let _guard = NoErrorDialogs::new();
    let root = format!("{letter}:\\");
    let wide = to_wide(root.as_ref()).ok()?;
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let kind = match unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } {
        DRIVE_NO_ROOT_DIR => return None,
        DRIVE_FIXED => DriveKind::Fixed,
        DRIVE_REMOVABLE => DriveKind::Removable,
        DRIVE_REMOTE => DriveKind::Network,
        DRIVE_CDROM => DriveKind::Optical,
        DRIVE_RAMDISK => DriveKind::Ram,
        _ => DriveKind::Unknown,
    };
    let mut label = [0u16; 261];
    // SAFETY: `wide` is NUL-terminated; the label buffer is a local slice whose length is passed.
    let has_media = unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            Some(&mut label),
            None,
            None,
            None,
            None,
        )
    }
    .is_ok();
    let label = if has_media {
        let len = label.iter().position(|&c| c == 0).unwrap_or(label.len());
        String::from_utf16_lossy(&label[..len])
    } else {
        String::new()
    };
    let space = has_media.then(|| space(&wide)).flatten();
    Some(Drive {
        root: PathBuf::from(root),
        letter,
        kind,
        label,
        space,
    })
}

fn space(root: &[u16]) -> Option<Space> {
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `root` is NUL-terminated; both out-pointers refer to locals.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(root.as_ptr()),
            Some(&raw mut free),
            Some(&raw mut total),
            None,
        )
    }
    .ok()?;
    Some(Space { total, free })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_drive_is_listed_with_space() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let letter = system.chars().next().unwrap();
        let all = drives();
        let drive = all.iter().find(|d| d.letter == letter).unwrap();
        assert_eq!(drive.kind, DriveKind::Fixed);
        let space = drive.space.unwrap();
        assert!(space.total > 0 && space.free <= space.total);
        assert!((0.0..=1.0).contains(&space.used_fraction()));
    }

    #[test]
    fn unmounted_letter_is_none() {
        // Some letter is always free on a test machine.
        let free = (b'A'..=b'Z').map(char::from).find(|&c| drive(c).is_none());
        assert!(free.is_some());
    }
}
