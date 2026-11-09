//! The shell's own context menu (`IContextMenu`, FEATURES.md F-CTX-02, SYSTEM-INTEGRATION.md),
//! for items of one folder or for a folder's empty area, third-party entries (7-Zip, VS Code)
//! included. While the menu is tracked, menu messages sent to the owner window are forwarded to
//! `IContextMenu3`/`IContextMenu2` through a window subclass, so "Send to" and "Open with" fill in
//! and draw their icons.
//!
//! Two steps, so a slow path (an offline network share) never freezes the window:
//! 1. [`prepare`] parses the paths into PIDLs on a background thread.
//! 2. [`show`] (or [`invoke`]) binds them, builds the menu and tracks it on the UI thread (an
//!    STA), through [`crate::modal::run`]. The shell objects never leave that thread.
//!
//! Rename, Delete and Paste are not run by the shell: they come back as [`OwnVerb`] so Switch
//! Explorer runs them itself, with its journal, undo and inline rename. Other verbs may run
//! asynchronously (`CMIC_MASK_ASYNCOK`), so a long one does not hold the window.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
// CMIC_MASK_UNICODE and CMIC_MASK_ASYNCOK have the values of SEE_MASK_UNICODE and
// SEE_MASK_ASYNCOK (shobjidl.h); windows-rs exports only the latter.
use windows::Win32::UI::Shell::{
    CMF_CANRENAME, CMF_EXPLORE, CMF_EXTENDEDVERBS, CMF_ITEMMENU, CMF_NORMAL, CMIC_MASK_PTINVOKE,
    CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, DefSubclassProc, GCS_VERBW, IContextMenu,
    IContextMenu2, IContextMenu3, ILClone, ILFindLastID, ILFree, ILIsParent, ILRemoveLastID,
    IShellFolder, RemoveWindowSubclass, SEE_MASK_ASYNCOK as CMIC_MASK_ASYNCOK,
    SEE_MASK_UNICODE as CMIC_MASK_UNICODE, SHBindToObject, SHBindToParent, SHParseDisplayName,
    SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, GetWindowRect, HMENU, SW_SHOWNORMAL,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_DRAWITEM,
    WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR,
};
use windows::core::{HRESULT, Interface as _, PCSTR, PCWSTR, PSTR};

use crate::modal::guard;
use crate::{ShellError, StaGuard};

const SUBCLASS_ID: usize = 0x5358_4D4E;
const FIRST_CMD: u32 = 1;
const LAST_CMD: u32 = 0x7FFF;
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the user cancelled the verb's own dialog.
const CANCELLED: HRESULT = HRESULT(0x8007_04C7_u32 as i32);
/// The classic path limit of `SHParseDisplayName`.
const MAX_PATH: usize = 260;

/// What the menu is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// Items of one folder (the shell's menu needs a common parent).
    Items(Vec<PathBuf>),
    /// The empty area of a folder.
    Background(PathBuf),
}

/// A command Switch Explorer runs itself instead of the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnVerb {
    Rename,
    Delete,
    /// Paste into this folder (the background's folder, or the folder item the menu was for).
    Paste(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuOutcome {
    /// Closed without a choice, or the chosen command's own dialog was cancelled.
    Dismissed,
    /// The shell ran the command; `verb` is empty for an entry that has none.
    Invoked {
        verb: String,
    },
    Own(OwnVerb),
}

/// How to show the menu.
#[derive(Debug, Clone, Copy)]
pub struct MenuOptions {
    /// The window that owns the menu (the Switch Explorer window).
    pub owner: isize,
    /// Where, in physical client pixels; `None` = at the pointer when it is over the window,
    /// else the window's centre.
    pub point: Option<(i32, i32)>,
    /// `Shift`: the extended verbs too ("Copy as path"…).
    pub extended: bool,
    /// Draw the menu dark or light (`integration.native_menu_dark`); `None` leaves it as the
    /// system draws it.
    pub dark: Option<bool>,
}

/// A menu target parsed into PIDLs, ready for [`show`]. Plain shell memory: it may be made on
/// any thread and used on another.
#[derive(Debug)]
pub struct PreparedMenu {
    target: MenuTarget,
    /// Absolute PIDLs of the items, or the one of the background folder.
    pidls: Vec<Pidl>,
}

impl PreparedMenu {
    pub fn target(&self) -> &MenuTarget {
        &self.target
    }
}

/// Parses `target` for [`show`]. Blocking (a network path can take seconds): call it on a
/// background thread, which it joins to an STA for the call.
pub fn prepare(target: MenuTarget) -> Result<PreparedMenu, ShellError> {
    let _com = StaGuard::new()?;
    let pidls = match &target {
        MenuTarget::Items(paths) if paths.is_empty() => {
            return Err(ShellError::InvalidPath(String::new()));
        }
        MenuTarget::Items(paths) => paths
            .iter()
            .map(|p| Pidl::parse(p))
            .collect::<Result<Vec<_>, _>>()?,
        MenuTarget::Background(folder) => vec![Pidl::parse(folder)?],
    };
    if let MenuTarget::Items(paths) = &target
        && !share_parent(&pidls)
    {
        return Err(ShellError::InvalidPath(format!(
            "{} items of a shell menu must share a folder",
            paths.len()
        )));
    }
    Ok(PreparedMenu { target, pidls })
}

/// Shows the shell menu and runs the chosen command (or hands back an own one). Call on the
/// UI thread through the modal host.
pub fn show(prepared: &PreparedMenu, options: MenuOptions) -> Result<MenuOutcome, ShellError> {
    let owner = HWND(options.owner as *mut core::ffi::c_void);
    let menu = context_menu(prepared, owner)?;
    let items = matches!(prepared.target, MenuTarget::Items(_));
    // SAFETY: the shell objects are created and used on this (UI, STA) thread only; the menu
    // handle is released by its guard; the subclass is removed (guard dropped) before `state`.
    unsafe {
        let hmenu = CreatePopupMenu().map_err(ShellError::Com)?;
        let _hmenu = MenuGuard(hmenu);
        let mut flags = CMF_NORMAL | CMF_EXPLORE;
        if items {
            // Without these the shell leaves Rename out.
            flags |= CMF_CANRENAME | CMF_ITEMMENU;
        }
        if options.extended {
            flags |= CMF_EXTENDEDVERBS;
        }
        menu.QueryContextMenu(hmenu, 0, FIRST_CMD, LAST_CMD, flags)
            .ok()
            .map_err(ShellError::Com)?;
        let state = MenuState {
            cm3: menu.cast().ok(),
            cm2: menu.cast().ok(),
        };
        // The modal host runs one job at a time, so the fixed subclass id is never nested.
        let subclass = SubclassGuard::install(owner, &state);
        if subclass.is_none() {
            tracing::warn!("menu: no subclass; submenus may stay empty");
        }
        if let Some(dark) = options.dark {
            set_menu_mode(if dark { MODE_DARK } else { MODE_LIGHT });
        }
        let pt = menu_point(owner, options.point);
        // Without this the menu does not close when the user clicks elsewhere.
        let _ = SetForegroundWindow(owner);
        let selected = TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
            pt.x,
            pt.y,
            owner,
            None,
        );
        // The mode is process-wide: back to the default, so dialogs and later menus (the
        // switch turned off) draw as the system does.
        if options.dark.is_some() {
            set_menu_mode(MODE_DEFAULT);
        }
        drop(subclass);
        drop(state);
        let Some(offset) = u32::try_from(selected.0)
            .ok()
            .filter(|&id| id >= FIRST_CMD)
            .map(|id| (id - FIRST_CMD) as usize)
        else {
            return Ok(MenuOutcome::Dismissed);
        };
        let verb = command_verb(&menu, offset);
        if let Some(own) = own_verb(&verb, &prepared.target) {
            return Ok(MenuOutcome::Own(own));
        }
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | CMIC_MASK_ASYNCOK,
            hwnd: owner,
            // A command offset in the low word stands for the verb (MAKEINTRESOURCE).
            lpVerb: PCSTR(offset as *const u8),
            lpVerbW: PCWSTR(offset as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: pt,
            ..Default::default()
        };
        match menu.InvokeCommand((&raw const info).cast::<CMINVOKECOMMANDINFO>()) {
            Ok(()) => Ok(MenuOutcome::Invoked { verb }),
            Err(err) if err.code() == CANCELLED => Ok(MenuOutcome::Dismissed),
            Err(err) => Err(ShellError::Com(err)),
        }
    }
}

/// Runs a canonical verb of the shell menu without showing it (`properties`). Call on the UI
/// thread through the modal host.
pub fn invoke(prepared: &PreparedMenu, owner: isize, verb: &str) -> Result<(), ShellError> {
    let owner = HWND(owner as *mut core::ffi::c_void);
    let menu = context_menu(prepared, owner)?;
    let ansi = CString::new(verb).map_err(|_| ShellError::InvalidPath(verb.to_string()))?;
    let wide: Vec<u16> = verb.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: as in `show`; `ansi` and `wide` are NUL-terminated and outlive the call.
    unsafe {
        let hmenu = CreatePopupMenu().map_err(ShellError::Com)?;
        let _hmenu = MenuGuard(hmenu);
        menu.QueryContextMenu(hmenu, 0, FIRST_CMD, LAST_CMD, CMF_NORMAL)
            .ok()
            .map_err(ShellError::Com)?;
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_UNICODE | CMIC_MASK_ASYNCOK,
            hwnd: owner,
            lpVerb: PCSTR(ansi.as_ptr().cast()),
            lpVerbW: PCWSTR(wide.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        match menu.InvokeCommand((&raw const info).cast::<CMINVOKECOMMANDINFO>()) {
            Err(err) if err.code() != CANCELLED => Err(ShellError::Com(err)),
            _ => Ok(()),
        }
    }
}

/// The verbs Switch Explorer runs itself.
fn own_verb(verb: &str, target: &MenuTarget) -> Option<OwnVerb> {
    match (verb.to_ascii_lowercase().as_str(), target) {
        ("rename", MenuTarget::Items(_)) => Some(OwnVerb::Rename),
        ("delete", MenuTarget::Items(_)) => Some(OwnVerb::Delete),
        ("paste", MenuTarget::Background(folder)) => Some(OwnVerb::Paste(folder.clone())),
        // Paste is offered on a single folder item: into that folder.
        ("paste", MenuTarget::Items(paths)) if paths.len() == 1 => {
            Some(OwnVerb::Paste(paths[0].clone()))
        }
        _ => None,
    }
}

/// The shell's menu object for a prepared target.
fn context_menu(prepared: &PreparedMenu, owner: HWND) -> Result<IContextMenu, ShellError> {
    let first = prepared
        .pidls
        .first()
        .ok_or_else(|| ShellError::InvalidPath(String::new()))?;
    match prepared.target {
        // SAFETY: every PIDL is alive (owned by `prepared`) for the whole block; the child
        // pointers point into them and the shell copies what it keeps; the folder and the menu
        // are used on this thread only.
        MenuTarget::Items(_) => unsafe {
            let folder: IShellFolder = SHBindToParent(first.0, None).map_err(ShellError::Com)?;
            let children: Vec<*const ITEMIDLIST> = prepared
                .pidls
                .iter()
                .map(|p| ILFindLastID(p.0).cast_const())
                .collect();
            folder
                .GetUIObjectOf(owner, &children, None)
                .map_err(ShellError::Com)
        },
        // SAFETY: the PIDL is alive for the calls; the objects stay on this thread.
        MenuTarget::Background(_) => unsafe {
            let folder: IShellFolder =
                SHBindToObject(None, first.0, None).map_err(ShellError::Com)?;
            folder.CreateViewObject(owner).map_err(ShellError::Com)
        },
    }
}

/// Whether every PIDL is a direct child of the first one's parent (by identity, not by path
/// text: `C:\` and `\\server\share` have no textual parent but different shell parents).
fn share_parent(pidls: &[Pidl]) -> bool {
    let Some(first) = pidls.first() else {
        return false;
    };
    if pidls.len() == 1 {
        return true;
    }
    // SAFETY: the clone is owned by `parent` (freed on drop); the PIDLs are alive.
    unsafe {
        let parent = Pidl(ILClone(first.0));
        if parent.0.is_null() || !ILRemoveLastID(Some(parent.0)).as_bool() {
            return false;
        }
        pidls
            .iter()
            .all(|p| ILIsParent(parent.0, p.0, true).as_bool())
    }
}

/// The screen point of the menu: the given client point, else the pointer if it is over the
/// window, else the window's centre.
fn menu_point(owner: HWND, client: Option<(i32, i32)>) -> POINT {
    // SAFETY: plain queries about our own window and the cursor, writing into locals.
    unsafe {
        if let Some((x, y)) = client {
            let mut pt = POINT { x, y };
            let _ = ClientToScreen(owner, &raw mut pt);
            return pt;
        }
        let mut rect = RECT::default();
        let _ = GetWindowRect(owner, &raw mut rect);
        let mut cursor = POINT::default();
        let inside = GetCursorPos(&raw mut cursor).is_ok()
            && cursor.x >= rect.left
            && cursor.x < rect.right
            && cursor.y >= rect.top
            && cursor.y < rect.bottom;
        if inside {
            cursor
        } else {
            POINT {
                x: i32::midpoint(rect.left, rect.right),
                y: i32::midpoint(rect.top, rect.bottom),
            }
        }
    }
}

struct MenuState {
    cm3: Option<IContextMenu3>,
    cm2: Option<IContextMenu2>,
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let handled = guard(None, || {
        if !matches!(
            msg,
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
        ) {
            return None;
        }
        // SAFETY: `data` points at the MenuState that lives until the subclass is removed.
        let state = unsafe { &*(data as *const MenuState) };
        if let Some(cm3) = &state.cm3 {
            let mut result = LRESULT(0);
            // SAFETY: forwards a menu message to the menu object that owns these items.
            if unsafe { cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&raw mut result)) }.is_ok() {
                return Some(result);
            }
        } else if let Some(cm2) = &state.cm2
            && msg != WM_MENUCHAR
        {
            // SAFETY: as above, for menus with only IContextMenu2 (which has no WM_MENUCHAR).
            if unsafe { cm2.HandleMenuMsg(msg, wparam, lparam) }.is_ok() {
                // WM_MEASUREITEM and WM_DRAWITEM answer TRUE once handled.
                let answer = isize::from(msg != WM_INITMENUPOPUP);
                return Some(LRESULT(answer));
            }
        }
        None
    });
    match handled {
        Some(result) => result,
        // SAFETY: default processing for everything we do not handle.
        None => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

fn command_verb(menu: &IContextMenu, offset: usize) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: GCS_VERBW writes a NUL-terminated UTF-16 string of at most `buf.len()` units.
    let ok = unsafe {
        menu.GetCommandString(
            offset,
            GCS_VERBW,
            None,
            PSTR(buf.as_mut_ptr().cast()),
            buf.len() as u32,
        )
    }
    .is_ok();
    if !ok {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// `SetPreferredAppMode` values.
const MODE_DEFAULT: i32 = 0;
const MODE_DARK: i32 = 2;
const MODE_LIGHT: i32 = 3;

/// Dark or light popup menus through uxtheme's undocumented ordinals 135/136
/// (`SetPreferredAppMode`, `FlushMenuThemes`), as Windows' own apps do. Missing entry points
/// are ignored. The mode is process-wide; [`show`] sets it back to the default afterwards.
fn set_menu_mode(mode: i32) {
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();
    // Loaded once from System32 and kept for the life of the process.
    static UXTHEME: OnceLock<isize> = OnceLock::new();
    let module = *UXTHEME.get_or_init(|| {
        // SAFETY: loads a system DLL from System32 only (no DLL planting).
        unsafe {
            LoadLibraryExW(
                windows::core::w!("uxtheme.dll"),
                Some(HANDLE::default()),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map_or(0, |m| m.0 as isize)
    });
    if module == 0 {
        return;
    }
    // SAFETY: each entry point is checked before use; the signatures match uxtheme's exports
    // (ordinal 135 takes the mode, 136 takes nothing).
    unsafe {
        let module = HMODULE(module as *mut core::ffi::c_void);
        if let Some(f) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            let f: SetPreferredAppMode = std::mem::transmute(f);
            f(mode);
        }
        if let Some(f) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            let f: FlushMenuThemes = std::mem::transmute(f);
            f();
        }
    }
}

/// Installs the menu-message subclass on the owner and removes it on drop.
struct SubclassGuard(HWND);

impl SubclassGuard {
    /// # Safety
    /// `state` must outlive the returned guard.
    unsafe fn install(owner: HWND, state: &MenuState) -> Option<Self> {
        // SAFETY: subclassing our own window on its thread; the reference data outlives the
        // guard (the caller's contract).
        let ok = unsafe {
            SetWindowSubclass(
                owner,
                Some(subclass_proc),
                SUBCLASS_ID,
                std::ptr::from_ref(state) as usize,
            )
        };
        ok.as_bool().then_some(Self(owner))
    }
}

impl Drop for SubclassGuard {
    fn drop(&mut self) {
        // SAFETY: removes exactly the subclass installed by `install`.
        let _ = unsafe { RemoveWindowSubclass(self.0, Some(subclass_proc), SUBCLASS_ID) };
    }
}

/// An absolute PIDL, freed on drop.
#[derive(Debug)]
pub(crate) struct Pidl(pub(crate) *mut ITEMIDLIST);

// SAFETY: a PIDL is a block of task-allocator memory with no thread affinity; it is owned by
// one `Pidl` at a time and freed once.
unsafe impl Send for Pidl {}

impl Pidl {
    pub(crate) fn parse(path: &Path) -> Result<Self, ShellError> {
        let path = shell_path(path)?;
        let wide = sx_win::to_wide(path.as_os_str())
            .map_err(|err| ShellError::InvalidPath(err.to_string()))?;
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the PIDL is owned by the
        // returned guard.
        unsafe { SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &raw mut pidl, 0, None) }
            .map_err(|source| ShellError::Item {
                path: path.clone(),
                source,
            })?;
        Ok(Self(pidl))
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the PIDL came from the shell's task allocator and is freed once.
            unsafe { ILFree(Some(self.0.cast_const())) };
        }
    }
}

/// The form `SHParseDisplayName` accepts: without a verbatim prefix (`\\?\C:\…` → `C:\…`,
/// `\\?\UNC\server\share` → `\\server\share`), within the classic path limit.
fn shell_path(path: &Path) -> Result<PathBuf, ShellError> {
    let text = path.as_os_str().to_string_lossy();
    let plain = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    };
    if plain.as_os_str().len() >= MAX_PATH {
        return Err(ShellError::InvalidPath(format!(
            "a path of {} characters is too long for the shell",
            plain.as_os_str().len()
        )));
    }
    Ok(plain)
}

struct MenuGuard(HMENU);

impl Drop for MenuGuard {
    fn drop(&mut self) {
        // SAFETY: the menu was created by CreatePopupMenu and is destroyed once.
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_dropped() {
        assert_eq!(
            shell_path(Path::new(r"\\?\C:\Users\Raka")).unwrap(),
            Path::new(r"C:\Users\Raka")
        );
        assert_eq!(
            shell_path(Path::new(r"\\?\UNC\server\share\x")).unwrap(),
            Path::new(r"\\server\share\x")
        );
        assert_eq!(
            shell_path(Path::new(r"D:\Data")).unwrap(),
            Path::new(r"D:\Data")
        );
        let long = format!(r"C:\{}", "a".repeat(300));
        assert!(shell_path(Path::new(&long)).is_err());
    }

    #[test]
    fn own_verbs() {
        let items = MenuTarget::Items(vec![PathBuf::from(r"C:\x\folder")]);
        let two = MenuTarget::Items(vec![PathBuf::from(r"C:\x\a"), PathBuf::from(r"C:\x\b")]);
        let background = MenuTarget::Background(PathBuf::from(r"C:\x"));
        assert_eq!(own_verb("Rename", &items), Some(OwnVerb::Rename));
        assert_eq!(own_verb("delete", &two), Some(OwnVerb::Delete));
        assert_eq!(
            own_verb("paste", &background),
            Some(OwnVerb::Paste(PathBuf::from(r"C:\x")))
        );
        assert_eq!(
            own_verb("paste", &items),
            Some(OwnVerb::Paste(PathBuf::from(r"C:\x\folder")))
        );
        assert_eq!(own_verb("paste", &two), None);
        assert_eq!(own_verb("properties", &items), None);
        assert_eq!(own_verb("delete", &background), None);
    }
}
