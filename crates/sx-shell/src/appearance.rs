//! The Windows app theme (light/dark) and the "Transparency effects" switch, read from
//! `HKCU\...\Themes\Personalize` and watched with `RegNotifyChangeKeyValue`, which fires at once;
//! `WM_SETTINGCHANGE` arrives ~600 ms late (ARCHITECTURE.md §8.4). Read-only: nothing is written.

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_QUERY_VALUE, REG_NOTIFY_CHANGE_LAST_SET,
    RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
use windows::core::{PCWSTR, w};

const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsAppearance {
    /// `AppsUseLightTheme` (a missing value means light, as Windows treats it).
    pub light: bool,
    /// `EnableTransparency` (a missing value means on).
    pub transparency: bool,
}

impl Default for OsAppearance {
    fn default() -> Self {
        Self {
            light: true,
            transparency: true,
        }
    }
}

/// A read/notify handle on the Personalize key, closed on drop.
struct Key(HKEY);

impl Key {
    fn open() -> Option<Self> {
        let mut key = HKEY::default();
        // SAFETY: opens a read/notify handle owned by the returned guard.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PERSONALIZE,
                Some(0),
                KEY_NOTIFY | KEY_QUERY_VALUE,
                &raw mut key,
            )
        };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    fn dword(&self, name: PCWSTR) -> Option<u32> {
        let mut value: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: reads a DWORD into a local with its size declared; `name` is a static string.
        let status = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                name,
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&raw mut size),
            )
        };
        (status == ERROR_SUCCESS).then_some(value)
    }

    fn appearance(&self) -> OsAppearance {
        OsAppearance {
            light: self.dword(w!("AppsUseLightTheme")).is_none_or(|v| v != 0),
            transparency: self.dword(w!("EnableTransparency")).is_none_or(|v| v != 0),
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: closes the handle opened in `open`.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// The current appearance (defaults when the key cannot be read).
pub fn current() -> OsAppearance {
    Key::open().map(|key| key.appearance()).unwrap_or_default()
}

/// Calls `on_change` on a dedicated thread every time the appearance changes, until it returns
/// `false`. The thread sleeps in the kernel between changes (no polling) and notices that it
/// should stop when `on_change` says so. The notification is armed before each read, so a change
/// made while the previous one is handled is not missed.
pub fn watch(mut on_change: impl FnMut(OsAppearance) -> bool + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("sx-appearance".into())
        .spawn(move || {
            let Some(key) = Key::open() else {
                tracing::warn!("appearance watch: cannot open the Personalize key");
                return;
            };
            // SAFETY: an unnamed auto-reset event, closed below when the thread ends.
            let Ok(changed) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
                tracing::warn!("appearance watch: cannot create an event");
                return;
            };
            let mut last = None;
            loop {
                // SAFETY: arms a one-shot notification on our own key handle, signalling our own
                // event; both outlive the wait below.
                let status = unsafe {
                    RegNotifyChangeKeyValue(
                        key.0,
                        false,
                        REG_NOTIFY_CHANGE_LAST_SET,
                        Some(changed),
                        true,
                    )
                };
                if status != ERROR_SUCCESS {
                    tracing::warn!(?status, "appearance watch stopped");
                    break;
                }
                let now = key.appearance();
                if last.is_some_and(|last| last != now) && !on_change(now) {
                    break;
                }
                last = Some(now);
                // SAFETY: waits on our own event.
                if unsafe { WaitForSingleObject(changed, INFINITE) } != WAIT_OBJECT_0 {
                    tracing::warn!("appearance watch wait failed");
                    break;
                }
            }
            // SAFETY: our own event, closed once.
            let _ = unsafe { CloseHandle(changed) };
        });
    if let Err(err) = spawned {
        tracing::error!(error = %err, "appearance watch thread failed to start");
    }
}
