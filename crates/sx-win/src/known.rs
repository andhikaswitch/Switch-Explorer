//! Known folders (user profile, Downloads, …) for Home and the default Pinned items.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Profile, FOLDERID_Videos, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath,
};
use windows::core::GUID;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnownFolder {
    Profile,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
}

impl KnownFolder {
    pub const PINNED_DEFAULTS: [Self; 6] = [
        Self::Desktop,
        Self::Downloads,
        Self::Documents,
        Self::Pictures,
        Self::Music,
        Self::Videos,
    ];

    fn id(self) -> &'static GUID {
        match self {
            Self::Profile => &FOLDERID_Profile,
            Self::Desktop => &FOLDERID_Desktop,
            Self::Documents => &FOLDERID_Documents,
            Self::Downloads => &FOLDERID_Downloads,
            Self::Pictures => &FOLDERID_Pictures,
            Self::Music => &FOLDERID_Music,
            Self::Videos => &FOLDERID_Videos,
        }
    }
}

/// The folder's current path (redirected folders included), without checking the disk: a folder
/// redirected to an offline share would otherwise block startup. `None` when the user has none.
pub fn known_folder(folder: KnownFolder) -> Option<PathBuf> {
    // SAFETY: the GUID is a static; no token means the current user. On success the returned
    // string is owned by us and freed with CoTaskMemFree below, after copying it.
    unsafe {
        let raw = SHGetKnownFolderPath(folder.id(), KF_FLAG_DONT_VERIFY, None).ok()?;
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.as_ptr().cast()));
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_and_downloads_exist() {
        let profile = known_folder(KnownFolder::Profile).unwrap();
        assert!(profile.is_dir());
        let downloads = known_folder(KnownFolder::Downloads).unwrap();
        assert!(downloads.is_absolute());
    }
}
