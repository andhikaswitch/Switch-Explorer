//! `shell:` names in the address bar (FEATURES.md F-NAV-03): `shell:Downloads`, `shell:startup`,
//! `shell:sendto`, … resolved by the shell to a file system folder.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use futures::channel::oneshot;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH};
use windows::core::PCWSTR;

use crate::com::StaGuard;
use crate::error::ShellError;

/// The folder a `shell:` name stands for (`name` without the prefix). `None` when the shell does
/// not know it or it is not a file system folder (e.g. `shell:RecycleBinFolder`).
pub fn resolve(name: String) -> impl Future<Output = Option<PathBuf>> + Send + 'static {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("sx-shell-name".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(|| resolve_blocking(&name).ok().flatten())
                .ok()
                .flatten();
            let _ = tx.send(result);
        });
    async move {
        spawned.ok()?;
        rx.await.ok().flatten()
    }
}

fn resolve_blocking(name: &str) -> Result<Option<PathBuf>, ShellError> {
    let _com = StaGuard::new()?;
    let parsing: Vec<u16> = format!("shell:{name}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `parsing` is NUL-terminated; the item and the string are released below.
    unsafe {
        let Ok(item) =
            SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(parsing.as_ptr()), None)
        else {
            return Ok(None);
        };
        let Ok(path) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
            return Ok(None);
        };
        let out = PathBuf::from(OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.as_ptr().cast()));
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names_resolve() {
        let downloads = futures::executor::block_on(resolve("Downloads".into())).unwrap();
        assert!(
            downloads.is_absolute() && downloads.is_dir(),
            "{downloads:?}"
        );
        assert_eq!(
            futures::executor::block_on(resolve("NoSuchFolder42".into())),
            None
        );
    }
}
