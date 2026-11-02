//! `cargo xtask cleanup-registry` — removes every Switch Explorer integration
//! (SYSTEM-INTEGRATION.md §16). Safe to run at any time:
//! - keys only we create (`switchexplorer` verbs, the `Run` value) are deleted when present;
//! - shared keys (Win+E, the dialog CLSIDs) are deleted only while their value still points at
//!   our executable or shim, so another application's registration is never touched.
//!
//! Uses `reg.exe` so xtask needs no FFI. Restoring values saved in `integration-backup.json`
//! joins when `sx_integration::registry` exists (Phase 5); until then nothing writes them.

use std::process::Command;

use anyhow::Context as _;

/// Keys created only by Switch Explorer.
const OWN_KEYS: &[&str] = &[
    r"HKCU\Software\Classes\Directory\shell\switchexplorer",
    r"HKCU\Software\Classes\Directory\shell\switchexplorer.open",
    r"HKCU\Software\Classes\Directory\Background\shell\switchexplorer.open",
    r"HKCU\Software\Classes\Drive\shell\switchexplorer",
    r"HKCU\Software\Classes\Drive\shell\switchexplorer.open",
];

/// Shared keys: deleted only when they reference us.
const SHARED_KEYS: &[&str] = &[
    // Win+E / "This PC" command.
    r"HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}",
    // FileOpenDialog / FileSaveDialog proxies (Picker Takeover), native and WOW64 views.
    r"HKCU\Software\Classes\CLSID\{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}",
    r"HKCU\Software\Classes\CLSID\{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}",
    r"HKCU\Software\Classes\WOW6432Node\CLSID\{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}",
    r"HKCU\Software\Classes\WOW6432Node\CLSID\{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}",
];

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "SwitchExplorer";
const MARKERS: &[&str] = &["switch-explorer.exe", "sx_picker_shim"];

pub fn cleanup(dry_run: bool) -> anyhow::Result<()> {
    let mut actions = 0;
    for key in OWN_KEYS {
        if query(key, &["/s"])?.is_some() {
            actions += 1;
            remove_key(key, dry_run)?;
        }
    }
    for key in SHARED_KEYS {
        match query(key, &["/s"])? {
            Some(dump) if MARKERS.iter().any(|m| dump.to_lowercase().contains(m)) => {
                actions += 1;
                remove_key(key, dry_run)?;
            }
            Some(_) => println!("kept {key} (belongs to another application)"),
            None => {}
        }
    }
    if query(RUN_KEY, &["/v", RUN_VALUE])?.is_some() {
        actions += 1;
        if dry_run {
            println!("would remove value {RUN_KEY}\\{RUN_VALUE}");
        } else {
            reg(&["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])?;
            println!("removed value {RUN_KEY}\\{RUN_VALUE}");
        }
    }
    if actions == 0 {
        println!("no Switch Explorer registry integration found; nothing to do");
    }
    Ok(())
}

fn remove_key(key: &str, dry_run: bool) -> anyhow::Result<()> {
    if dry_run {
        println!("would remove {key}");
    } else {
        reg(&["delete", key, "/f"])?;
        println!("removed {key}");
    }
    Ok(())
}

/// `reg query`; `None` when the key or value does not exist.
fn query(key: &str, extra: &[&str]) -> anyhow::Result<Option<String>> {
    let mut args = vec!["query", key];
    args.extend_from_slice(extra);
    let output = Command::new("reg")
        .args(&args)
        .output()
        .context("cannot run reg.exe")?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn reg(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("reg")
        .args(args)
        .status()
        .context("cannot run reg.exe")?;
    anyhow::ensure!(status.success(), "reg {} failed", args.join(" "));
    Ok(())
}
