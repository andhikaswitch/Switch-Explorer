//! Pinned sidebar folders (FEATURES.md F-SIDE-02), stored in `%APPDATA%\SwitchExplorer\pins.json`
//! as a list of paths. A missing file means the defaults (Desktop, Downloads, …).

use std::path::{Path, PathBuf};

use sx_win::KnownFolder;

pub(crate) fn defaults() -> Vec<PathBuf> {
    KnownFolder::PINNED_DEFAULTS
        .iter()
        .filter_map(|&folder| sx_win::known_folder(folder))
        .collect()
}

/// The saved pins, or the defaults when there is no (readable) file.
pub(crate) fn load(file: Option<&Path>) -> Vec<PathBuf> {
    file.and_then(|f| std::fs::read(f).ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<PathBuf>>(&bytes).ok())
        .unwrap_or_else(defaults)
}

/// Writes the pins atomically (temporary file, then rename).
pub(crate) fn save(file: &Path, pins: &[PathBuf]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(pins).map_err(std::io::Error::other)?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pins.json");
        assert_eq!(load(Some(&file)), defaults());
        let pins = vec![PathBuf::from(r"C:\Projects"), PathBuf::from(r"D:\写真")];
        save(&file, &pins).unwrap();
        assert_eq!(load(Some(&file)), pins);
        std::fs::write(&file, b"not json").unwrap();
        assert_eq!(load(Some(&file)), defaults());
    }
}
