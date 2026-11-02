//! Watches `HKCU\...\Themes\Personalize` read-only to timestamp the moment Windows writes
//! `AppsUseLightTheme` — the T0 of the light/dark latency measurement. Nothing is written.

use std::time::Instant;

use futures::channel::mpsc::UnboundedSender;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_QUERY_VALUE, REG_NOTIFY_CHANGE_LAST_SET,
    RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows::core::{PCWSTR, w};

#[derive(Debug)]
pub enum ThemeEvent {
    Changed { light: bool, at: Instant },
}

pub fn spawn(tx: UnboundedSender<ThemeEvent>) {
    let spawned = std::thread::Builder::new()
        .name("sx-theme-watch".into())
        .spawn(move || {
            let mut key = HKEY::default();
            // SAFETY: opens a read/notify handle that is closed at the end of the thread.
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                    Some(0),
                    KEY_NOTIFY | KEY_QUERY_VALUE,
                    &mut key,
                )
            };
            if opened != ERROR_SUCCESS {
                log::warn!(target: "sx_spike", "theme watch: cannot open key ({opened:?})");
                return;
            }
            let mut last = read_light(key);
            loop {
                // SAFETY: synchronous wait on our own key handle.
                let waited = unsafe {
                    RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false)
                };
                if waited != ERROR_SUCCESS {
                    break;
                }
                let at = Instant::now();
                let light = read_light(key);
                if light != last {
                    last = light;
                    if let Some(light) = light
                        && tx
                            .unbounded_send(ThemeEvent::Changed { light, at })
                            .is_err()
                    {
                        break;
                    }
                }
            }
            // SAFETY: closes the handle opened above.
            let _ = unsafe { RegCloseKey(key) };
        });
    if let Err(err) = spawned {
        log::error!(target: "sx_spike", "theme watch thread failed to start: {err}");
    }
}

fn read_light(key: HKEY) -> Option<bool> {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into a local with its size declared.
    let status = unsafe {
        RegGetValueW(
            key,
            PCWSTR::null(),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value != 0)
}
