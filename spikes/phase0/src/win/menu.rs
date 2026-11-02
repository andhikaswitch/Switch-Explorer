//! Native shell context menu (`IContextMenu`) for one item. While the menu is tracked, menu
//! messages sent to the GPUI window are forwarded to `IContextMenu3`/`IContextMenu2` through a
//! window subclass, so the "Send to" and "Open with" submenus fill in and draw their icons.

use std::fmt;
use std::path::Path;

use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
// CMIC_MASK_UNICODE has the same value as SEE_MASK_UNICODE (shobjidl.h); windows-rs only
// exports the latter.
use windows::Win32::UI::Shell::{
    CMF_EXPLORE, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO,
    CMINVOKECOMMANDINFOEX, DefSubclassProc, GCS_VERBW, IContextMenu, IContextMenu2, IContextMenu3,
    ILFree, IShellFolder, RemoveWindowSubclass, SEE_MASK_UNICODE as CMIC_MASK_UNICODE,
    SHBindToParent, SHParseDisplayName, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, HMENU, SW_SHOWNORMAL, SetForegroundWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_DRAWITEM, WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR,
};
use windows::core::{HSTRING, Interface as _, PCSTR, PCWSTR, PSTR, Result};

use super::Hwnd;

const SUBCLASS_ID: usize = 0x5358_4D4E;
const FIRST_CMD: u32 = 1;
const LAST_CMD: u32 = 0x7FFF;

#[derive(Debug)]
pub enum MenuOutcome {
    Dismissed,
    Invoked { verb: String },
    Skipped { verb: String, reason: &'static str },
}

impl fmt::Display for MenuOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dismissed => write!(f, "ditutup tanpa memilih"),
            Self::Invoked { verb } => write!(f, "perintah '{verb}' dijalankan"),
            Self::Skipped { verb, reason } => write!(f, "'{verb}' dilewati: {reason}"),
        }
    }
}

struct MenuState {
    cm3: Option<IContextMenu3>,
    cm2: Option<IContextMenu2>,
}

/// Shows the shell menu for `path` at `client` (physical client pixels) and runs the chosen verb.
/// Blocks in the menu loop: call it from a foreground task, outside GPUI's borrows.
pub fn show(
    hwnd: Hwnd,
    path: &Path,
    client: (i32, i32),
    extended: bool,
    dark: bool,
) -> Result<MenuOutcome> {
    let owner = hwnd.get();
    // SAFETY: shell COM objects are created and used on this (UI, STA) thread only; the PIDL
    // and menu are released by guards; the subclass is removed before `state` goes away.
    unsafe {
        let wide = HSTRING::from(path.as_os_str());
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(&wide, None::<&IBindCtx>, &mut pidl, 0, None)?;
        let _pidl = PidlGuard(pidl);
        let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
        let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child))?;
        let menu: IContextMenu = folder.GetUIObjectOf(owner, &[child.cast_const()], None)?;

        let hmenu = CreatePopupMenu()?;
        let _hmenu = MenuGuard(hmenu);
        let mut flags = CMF_NORMAL | CMF_EXPLORE;
        if extended {
            flags |= CMF_EXTENDEDVERBS;
        }
        menu.QueryContextMenu(hmenu, 0, FIRST_CMD, LAST_CMD, flags)
            .ok()?;

        let state = MenuState {
            cm3: menu.cast().ok(),
            cm2: menu.cast().ok(),
        };
        let _ = SetWindowSubclass(
            owner,
            Some(subclass_proc),
            SUBCLASS_ID,
            &raw const state as usize,
        );
        apply_menu_theme(dark);
        let mut pt = POINT {
            x: client.0,
            y: client.1,
        };
        let _ = ClientToScreen(owner, &mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(owner);
        let selected = TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
            pt.x,
            pt.y,
            owner,
            None,
        );
        let _ = RemoveWindowSubclass(owner, Some(subclass_proc), SUBCLASS_ID);
        drop(state);

        let Ok(id) = u32::try_from(selected.0) else {
            return Ok(MenuOutcome::Dismissed);
        };
        if id < FIRST_CMD {
            return Ok(MenuOutcome::Dismissed);
        }
        let offset = (id - FIRST_CMD) as usize;
        let verb = command_verb(&menu, offset);
        if verb.eq_ignore_ascii_case("rename") {
            return Ok(MenuOutcome::Skipped {
                verb,
                reason: "rename butuh view Explorer; Switch Explorer menanganinya sendiri",
            });
        }
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
            hwnd: owner,
            lpVerb: PCSTR(offset as *const u8),
            lpVerbW: PCWSTR(offset as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: pt,
            ..Default::default()
        };
        menu.InvokeCommand((&raw const info).cast::<CMINVOKECOMMANDINFO>())?;
        Ok(MenuOutcome::Invoked {
            verb: if verb.is_empty() {
                format!("#{offset}")
            } else {
                verb
            },
        })
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let handled = super::guard(None, || {
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
            // SAFETY: forwarding a menu message to the menu object that owns these items.
            if unsafe { cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }.is_ok() {
                return Some(result);
            }
        } else if let Some(cm2) = &state.cm2 {
            // SAFETY: as above, for menus that only implement IContextMenu2.
            if unsafe { cm2.HandleMenuMsg(msg, wparam, lparam) }.is_ok() {
                return Some(LRESULT(0));
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
    // SAFETY: GCS_VERBW writes a NUL-terminated UTF-16 string into our buffer.
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

/// Dark/light popup menus through uxtheme's undocumented ordinals (the same ones Windows and
/// gpui-component's NativeMenu use). Missing entry points are ignored.
fn apply_menu_theme(dark: bool) {
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();
    // SAFETY: uxtheme is loaded from System32 only; ordinals are checked for null before use.
    unsafe {
        let Ok(module) = LoadLibraryExW(
            windows::core::w!("uxtheme.dll"),
            Some(HANDLE::default()),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        ) else {
            return;
        };
        let module: HMODULE = module;
        if let Some(f) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            let f: SetPreferredAppMode = std::mem::transmute(f);
            f(if dark { 2 } else { 3 });
        }
        if let Some(f) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            let f: FlushMenuThemes = std::mem::transmute(f);
            f();
        }
    }
}

struct PidlGuard(*mut ITEMIDLIST);

impl Drop for PidlGuard {
    fn drop(&mut self) {
        // SAFETY: the PIDL came from SHParseDisplayName and is freed once.
        unsafe { ILFree(Some(self.0.cast_const())) };
    }
}

struct MenuGuard(HMENU);

impl Drop for MenuGuard {
    fn drop(&mut self) {
        // SAFETY: the menu was created by CreatePopupMenu and is destroyed once.
        let _ = unsafe { DestroyMenu(self.0) };
    }
}
