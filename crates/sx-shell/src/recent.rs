//! The files the user opened last (FEATURES.md F-HOME-04), from Windows' own Recent Items:
//! the shortcuts in `%APPDATA%\Microsoft\Windows\Recent`. Nothing is tracked by this app.
//!
//! A shortcut's target and whether it is a folder are read from the shortcut itself, never by
//! asking the target: a shortcut to a share that is gone must not stall the list. Only targets
//! on fixed local drives are checked for still being there. (That check follows a junction in
//! the path, and the Recent folder itself can be redirected to a share: both can be slow, which
//! is why this runs on its own thread and Home shows what it has meanwhile.)
//!
//! Any app can put a shortcut in that folder, so a target is only listed when it is a plain
//! document path: programs, scripts and shortcuts are left out (one click on a row opens it),
//! and so are device and verbatim paths. Callers must not ask a listed file for its own icon or
//! properties without the user acting on it: a target on a share would be contacted (use the
//! icon of its type).

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Component, Path, PathBuf, Prefix};
use std::time::SystemTime;

use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, GetDriveTypeW, WIN32_FIND_DATAW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Recent, IShellLinkW, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{Interface as _, PCWSTR};

use crate::error::ShellError;

/// `GetDriveTypeW`: no volume at that letter (an unplugged drive).
const DRIVE_NO_ROOT_DIR: u32 = 1;
/// `GetDriveTypeW`: a disk that is part of this PC.
const DRIVE_FIXED: u32 = 3;
/// Room for a shortcut's target. `IShellLinkW::GetPath` gives at most `MAX_PATH` (260) units;
/// a longer target comes back empty and is left out.
const PATH_UNITS: usize = 1024;
/// Shortcuts looked at for one list, newest first. Folders, programs, dead targets and repeats
/// among them are dropped, so far more are read than are shown; reading one takes milliseconds
/// and the loop stops once the list is full.
const LOOK_AT: usize = 300;
/// Targets that run when opened: never listed, whatever put their shortcut there.
const RUNS: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "scr",
    "pif",
    "msi",
    "msp",
    "lnk",
    "url",
    "ps1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "cpl",
    "jar",
    "reg",
    "appref-ms",
];

/// A recently used file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFile {
    pub path: PathBuf,
    /// When it was last opened (when Windows last wrote its shortcut).
    pub used: SystemTime,
}

/// The newest `limit` recently used files, newest first. Blocking (reads a folder and its
/// shortcuts); enters an STA: call it off the UI thread.
pub fn recent_files(limit: usize) -> Result<Vec<RecentFile>, ShellError> {
    let _com = crate::com::StaGuard::new()?;
    let Some(folder) = recent_folder() else {
        return Ok(Vec::new());
    };
    recent_in(&folder, limit)
}

/// The Recent Items folder, without checking the disk.
fn recent_folder() -> Option<PathBuf> {
    // SAFETY: the GUID is a static; no token means the current user. On success the returned
    // string is ours: it is copied, then freed with CoTaskMemFree, with nothing in between
    // that could return early.
    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_Recent, KF_FLAG_DONT_VERIFY, None).ok()?;
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.as_ptr().cast()));
        Some(path)
    }
}

/// The recent files the shortcuts in `folder` point to. Needs an STA on the calling thread.
fn recent_in(folder: &Path, limit: usize) -> Result<Vec<RecentFile>, ShellError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut links: Vec<(PathBuf, SystemTime)> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            })
            .filter_map(|entry| Some((entry.path(), entry.metadata().ok()?.modified().ok()?)))
            .collect(),
        // No Recent Items folder (never used, or cleared by policy): nothing recent.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(ShellError::Io(err)),
    };
    links.sort_by_key(|(_, used)| std::cmp::Reverse(*used));
    links.truncate(LOOK_AT);

    // SAFETY: creates the shell's own in-process link object on this thread's STA.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(ShellError::Com)?;
    let file: IPersistFile = link.cast().map_err(ShellError::Com)?;
    let mut buffer = vec![0u16; PATH_UNITS];
    let mut drives = Drives::default();
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(limit);
    // When no shortcut at all can be read, that is a failure, not "nothing recent".
    let (mut read, mut unreadable) = (0usize, None);
    for (shortcut, used) in links {
        let target = match target_of(&link, &file, &shortcut, &mut buffer) {
            Ok(target) => {
                read += 1;
                target
            }
            Err(err) => {
                // The code only: a shortcut's name says what the user opened.
                tracing::trace!(code = ?err.code(), "recent shortcut not read");
                unreadable = Some(err);
                continue;
            }
        };
        let Some(target) = target.filter(|target| listable(&target.path)) else {
            continue;
        };
        if target.folder || !seen.insert(target.path.to_string_lossy().to_lowercase()) {
            continue;
        }
        match drives.kind_of(&target.path) {
            // Its drive is not there now (unplugged): known without asking the target.
            Some(DRIVE_NO_ROOT_DIR) => continue,
            // On a local disk the target is asked: gone, or a folder after all, is not listed.
            Some(DRIVE_FIXED) => match std::fs::symlink_metadata(&target.path) {
                Ok(found) if found.is_dir() => continue,
                Ok(_) => {}
                Err(_) => continue,
            },
            // Anything else (a share, a removable or network drive) is not asked: it could hang.
            _ => {}
        }
        out.push(RecentFile {
            path: target.path,
            used,
        });
        if out.len() == limit {
            break;
        }
    }
    match unreadable {
        Some(err) if read == 0 => Err(ShellError::Com(err)),
        _ => Ok(out),
    }
}

/// Whether a shortcut's target may be listed: `X:\…` or `\\server\share\…` (not a device or
/// verbatim path), without `.` or `..`, and not something that runs when opened.
fn listable(path: &Path) -> bool {
    let mut parts = path.components();
    let plain_root = matches!(
        parts.next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::UNC(..))
    ) && matches!(parts.next(), Some(Component::RootDir));
    let clean = parts.all(|part| matches!(part, Component::Normal(_)));
    let runs = path.extension().is_some_and(|ext| {
        let ext = ext.to_string_lossy().to_lowercase();
        RUNS.contains(&ext.as_str())
    });
    plain_root && clean && path.file_name().is_some() && !runs
}

struct Target {
    path: PathBuf,
    folder: bool,
}

/// What a shortcut points to, as the shortcut records it: `Ok(None)` when it points to
/// something without a file system path (or a path too long for `GetPath`), `Err` when the
/// shortcut cannot be read.
fn target_of(
    link: &IShellLinkW,
    file: &IPersistFile,
    shortcut: &Path,
    buffer: &mut [u16],
) -> windows::core::Result<Option<Target>> {
    let wide: Vec<u16> = shortcut
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    buffer.fill(0);
    let mut found = WIN32_FIND_DATAW::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `buffer` and `found` are valid
    // for writes for the whole call. `Load` replaces what the link object held (a failed load
    // leaves it empty, and `GetPath` is not called after one). `GetPath` with no flags only
    // reads what the shortcut stores: it does not resolve, expand or touch the target, so a
    // dead share costs nothing. `Resolve`, `SetPath` and `SetIDList` must never be called
    // here: they do contact the target (and block for tens of seconds when it is gone).
    unsafe {
        file.Load(PCWSTR(wide.as_ptr()), STGM_READ)?;
        link.GetPath(buffer, &raw mut found, 0)?;
    }
    // A shortcut without a path answers S_FALSE (a success) with an empty string: the length
    // is what tells.
    let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(0);
    if len == 0 {
        return Ok(None);
    }
    Ok(Some(Target {
        path: PathBuf::from(OsString::from_wide(&buffer[..len])),
        // As recorded when the shortcut was written (zero when the target was not there then).
        folder: found.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
    }))
}

/// What kind of drive each letter is (`GetDriveTypeW`), asked once per letter. Asking does not
/// touch the drive, mapped network drives included.
#[derive(Default)]
struct Drives {
    known: [Option<u32>; 26],
}

impl Drives {
    /// The drive type of an `X:\…` path; `None` for any other path (a share).
    fn kind_of(&mut self, path: &Path) -> Option<u32> {
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        if bytes.len() < 3
            || bytes[1] != b':'
            || bytes[2] != b'\\'
            || !bytes[0].is_ascii_alphabetic()
        {
            return None;
        }
        let letter = bytes[0].to_ascii_uppercase();
        Some(
            *self.known[usize::from(letter - b'A')].get_or_insert_with(|| {
                let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
                // SAFETY: `root` is a NUL-terminated drive root that outlives the call.
                unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a shortcut to `target` as `name` in `folder`.
    fn shortcut(folder: &Path, name: &str, target: &Path) {
        let wide = |path: &Path| -> Vec<u16> {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        // SAFETY: test-only; the strings are NUL-terminated and outlive the calls, on this
        // thread's STA.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(wide(target).as_ptr())).unwrap();
            let file: IPersistFile = link.cast().unwrap();
            file.Save(PCWSTR(wide(&folder.join(name)).as_ptr()), true)
                .unwrap();
        }
    }

    fn names(listed: &[RecentFile]) -> Vec<String> {
        listed
            .iter()
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_lowercase()
            })
            .collect()
    }

    #[test]
    fn lists_files_newest_first_without_folders_repeats_or_dead_targets() {
        let _com = crate::com::StaGuard::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (recent, data) = (dir.path().join("Recent"), dir.path().join("data"));
        std::fs::create_dir_all(&recent).unwrap();
        std::fs::create_dir_all(data.join("folder")).unwrap();
        for name in ["old.txt", "new.txt", "gone.txt", "tool.exe", "run.bat"] {
            std::fs::write(data.join(name), b"x").unwrap();
        }
        shortcut(&recent, "old.txt.lnk", &data.join("old.txt"));
        shortcut(&recent, "folder.lnk", &data.join("folder"));
        shortcut(&recent, "gone.txt.lnk", &data.join("gone.txt"));
        // Programs and scripts are never listed, whatever put their shortcuts here.
        shortcut(&recent, "tool.exe.lnk", &data.join("tool.exe"));
        shortcut(&recent, "run.bat.lnk", &data.join("run.bat"));
        std::fs::write(recent.join("notes.txt"), b"not a shortcut").unwrap();
        std::fs::write(recent.join("broken.lnk"), b"not a shortcut either").unwrap();
        // Later shortcuts: `new.txt` twice (Windows keeps one per name, apps may add more).
        std::thread::sleep(std::time::Duration::from_millis(30));
        shortcut(&recent, "new.txt.lnk", &data.join("new.txt"));
        shortcut(&recent, "new (2).lnk", &data.join("NEW.txt"));
        std::fs::remove_file(data.join("gone.txt")).unwrap();

        let listed = names(&recent_in(&recent, 10).unwrap());
        // A temp folder on a fixed disk drops the dead target; elsewhere it stays unchecked.
        let on_fixed = Drives::default().kind_of(&data) == Some(DRIVE_FIXED);
        let mut expected = vec!["new.txt", "old.txt"];
        if !on_fixed {
            expected.push("gone.txt");
        }
        let mut sorted = listed.clone();
        sorted.sort();
        expected.sort_unstable();
        assert_eq!(sorted, expected);
        assert_eq!(listed.first().map(String::as_str), Some("new.txt"));
        assert_eq!(recent_in(&recent, 1).unwrap().len(), 1);
        assert_eq!(recent_in(&recent, 0).unwrap(), Vec::new());
    }

    /// Run by hand (`cargo test -p sx-shell -- --ignored a_share`): writing the test's shortcut
    /// asks the host and takes about 40 s; reading it back is what must stay instant.
    #[test]
    #[ignore = "writing a shortcut to a dead host takes ~40 s"]
    fn a_share_is_listed_without_being_asked() {
        let _com = crate::com::StaGuard::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // TEST-NET-1: nothing answers there. Reading the shortcut must not go and look.
        shortcut(
            dir.path(),
            "report.lnk",
            Path::new(r"\\192.0.2.77\share\report.docx"),
        );
        let started = std::time::Instant::now();
        let listed = recent_in(dir.path(), 5).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the share was contacted"
        );
        assert_eq!(names(&listed), ["report.docx"]);
    }

    #[test]
    fn only_unreadable_shortcuts_is_a_failure() {
        let _com = crate::com::StaGuard::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.lnk"), b"not a shortcut").unwrap();
        assert!(recent_in(dir.path(), 5).is_err());
        assert_eq!(
            recent_in(&dir.path().join("nope"), 5).unwrap(),
            Vec::new(),
            "no Recent folder is just nothing recent"
        );
    }

    #[test]
    fn only_plain_document_paths_are_listable() {
        assert!(listable(Path::new(r"C:\Docs\report.docx")));
        assert!(listable(Path::new(r"\\server\share\report.docx")));
        assert!(listable(Path::new(r"C:\Docs\no extension")));
        for path in [
            r"C:\Tools\setup.EXE",
            r"\\server\share\invoice.pdf.exe",
            r"C:\Docs\run.ps1",
            r"C:\Docs\link.lnk",
            r"\\?\C:\Docs\report.docx",
            r"\\.\PhysicalDrive0",
            r"relative\report.docx",
            r"C:report.docx",
            r"C:\Docs\..\secret.txt",
            r"C:\",
        ] {
            assert!(!listable(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn only_drive_paths_have_a_drive_kind() {
        let mut drives = Drives::default();
        assert_eq!(drives.kind_of(Path::new(r"\\server\share\a.txt")), None);
        assert_eq!(drives.kind_of(Path::new(r"relative\a.txt")), None);
        assert_eq!(drives.kind_of(Path::new(r"C:")), None);
    }
}
