//! A folder in another app: a terminal there (FEATURES.md F-OPS-12, `ops.terminal`) or VS Code.
//!
//! Plain process starts, off the UI thread. The folder goes in as the new process's working
//! directory, never inside a command line, so no path can be read as a command (a `;` in a
//! name would split a Windows Terminal command line).

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use gpui_kit::{Context, SharedString};
use rust_i18n::t;

use crate::workspace::Workspace;

/// A console program gets its own window, not ours (we have none).
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// The form a program takes as a working directory: without a verbatim prefix (`\\?\`).
fn plain(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Starts a terminal in `dir`. `choice` is `ops.terminal`: `auto` tries Windows Terminal, then
/// PowerShell; a named one is only that one.
fn start_terminal(dir: &Path, choice: &str) -> std::io::Result<()> {
    let dir = plain(dir);
    if matches!(choice, "auto" | "windows_terminal") {
        // `-d .`: the folder is the working directory we give it.
        match Command::new("wt.exe")
            .args(["-d", "."])
            .current_dir(&dir)
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(err) if choice == "windows_terminal" => return Err(err),
            Err(_) => {}
        }
    }
    let program = if choice == "cmd" {
        "cmd.exe"
    } else {
        "powershell.exe"
    };
    Command::new(program)
        .current_dir(&dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
}

/// Menus ask whether VS Code is installed: find out at start, off the UI thread.
pub(crate) fn warm_up(cx: &gpui_kit::App) {
    use gpui_kit::AppContext as _;
    cx.background_spawn(async {
        let _ = vscode();
    })
    .detach();
}

/// VS Code's program, when it is installed (per user or for everyone). Looked up once.
pub(crate) fn vscode() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let under = |var: &str| {
                std::env::var_os(var).map(|root| PathBuf::from(root).join("Microsoft VS Code"))
            };
            [
                std::env::var_os("LOCALAPPDATA").map(|root| {
                    PathBuf::from(root)
                        .join("Programs")
                        .join("Microsoft VS Code")
                }),
                under("ProgramFiles"),
                under("ProgramFiles(x86)"),
            ]
            .into_iter()
            .flatten()
            .map(|folder| folder.join("Code.exe"))
            .find(|exe| exe.is_file())
        })
        .as_deref()
}

impl Workspace {
    /// What "Open in Terminal" opens: the one selected folder, else the open folder.
    pub(crate) fn terminal_target(&self) -> Option<PathBuf> {
        let tab = self.tab();
        let folder = tab.location().as_path()?.to_path_buf();
        let selected = self.selected_paths();
        let one_folder = tab
            .selection
            .iter()
            .next()
            .and_then(|row| tab.snapshot()?.row(row as usize))
            .is_some_and(|entry| entry.kind.is_container());
        Some(match selected.as_slice() {
            [only] if one_folder => only.clone(),
            _ => folder,
        })
    }

    /// What "Open in VS Code" opens: the one selected item, else the open folder.
    pub(crate) fn code_target(&self) -> Option<PathBuf> {
        let folder = self.tab().location().as_path()?.to_path_buf();
        Some(match self.selected_paths().as_slice() {
            [only] => only.clone(),
            _ => folder,
        })
    }

    /// Opens a terminal in `dir` (`ops.terminal`); a failure says which and what to do.
    pub(crate) fn open_terminal(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let choice = self.features.ops_terminal;
        cx.spawn(async move |this, cx| {
            let started = crate::ops::off_thread(move || start_terminal(&dir, choice)).await;
            if let Some(Err(err)) = started {
                tracing::warn!(error = %err, choice, "terminal did not start");
                let _ = this.update(cx, |this, cx| {
                    // `auto` ends with PowerShell, so that is the one that failed.
                    let failed = match choice {
                        "windows_terminal" | "cmd" => choice,
                        _ => "powershell",
                    };
                    let panel = sx_settings::hotswitch::DEFAULT_BINDINGS
                        .iter()
                        .find(|b| b.action == sx_settings::HotSwitchAction::Panel)
                        .map(|b| crate::hotswitch::keys_text(b.keys))
                        .unwrap_or_default();
                    this.notice = Some(SharedString::from(t!(
                        "launch.terminal_failed",
                        name = t!(format!("launch.terminal_name.{failed}")),
                        keys = panel
                    )));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Opens `path` in VS Code, when it is installed.
    pub(crate) fn open_in_code(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(code) = vscode().map(Path::to_path_buf) else {
            self.notice = Some(t!("launch.code_missing").into());
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let target = plain(&path);
            let started =
                crate::ops::off_thread(move || Command::new(&code).arg(&target).spawn()).await;
            if let Some(Err(err)) = started {
                tracing::warn!(error = %err, "VS Code did not start");
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(SharedString::from(t!("launch.code_failed")));
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_get_paths_without_a_verbatim_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\Data")), PathBuf::from(r"C:\Data"));
        assert_eq!(
            plain(Path::new(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
        assert_eq!(
            plain(Path::new(r"D:\Foto gw")),
            PathBuf::from(r"D:\Foto gw")
        );
    }
}
