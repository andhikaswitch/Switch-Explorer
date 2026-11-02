//! OLE drag & drop (ADR-010): drag-out of real shell items, and an optional own drop target
//! that replaces GPUI's built-in one (the Phase 0 decision point). GPUI's target always answers
//! DROPEFFECT_COPY and only forwards paths; ours computes the effect from modifiers and drop
//! zones and reports which clipboard formats the source offers.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{Bounds, Pixels};
use windows::Win32::Foundation::{E_UNEXPECTED, HWND, LPARAM, POINT, POINTL, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IBindCtx,
    IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::GetClipboardFormatNameW;
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    IDropSource, IDropTarget, IDropTarget_Impl, MK_ALT, RegisterDragDrop, ReleaseStgMedium,
    RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, CLSID_DragDropHelper, DragQueryFileW, HDROP, IDropTargetHelper, ILFree,
    SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, PostMessageW, WM_LBUTTONUP};
use windows::core::{Error, HSTRING, Ref, Result, implement};

use super::{Hwnd, guard};

/// Drag-out in the spike allows copy and link only, so a drop on Explorer can never move (and
/// thereby take away) a fixture file by accident.
pub const SPIKE_ALLOWED: DROPEFFECT = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_LINK.0);

pub fn effect_name(effect: DROPEFFECT) -> &'static str {
    match effect {
        e if e == DROPEFFECT_NONE => "none (batal/ditolak)",
        e if e.0 & DROPEFFECT_MOVE.0 != 0 => "move",
        e if e.0 & DROPEFFECT_COPY.0 != 0 => "copy",
        e if e.0 & DROPEFFECT_LINK.0 != 0 => "link",
        _ => "lain",
    }
}

/// Starts a blocking OLE drag of `paths` and returns the effect the target performed.
/// Must run on the UI (STA) thread, from a foreground task outside GPUI's borrows.
pub fn start_drag(hwnd: Hwnd, paths: &[PathBuf], allowed: DROPEFFECT) -> Result<DROPEFFECT> {
    let pidls = PidlList::parse(paths)?;
    let ptrs: Vec<*const ITEMIDLIST> = pidls.0.iter().map(|p| p.cast_const()).collect();
    // SAFETY: the PIDLs are owned by `pidls` and outlive the calls; COM objects stay on this thread.
    unsafe {
        let items = SHCreateShellItemArrayFromIDLists(&ptrs)?;
        let data: IDataObject = items.BindToHandler(None::<&IBindCtx>, &BHID_DataObject)?;
        // A null drop source makes the shell supply its default one (Esc cancels, button-up drops).
        SHDoDragDrop(Some(hwnd.get()), &data, None::<&IDropSource>, allowed)
    }
}

/// DoDragDrop's modal loop swallows WM_LBUTTONUP, so GPUI would still see the button pressed.
pub fn post_button_up(hwnd: Hwnd) {
    let mut pt = POINT::default();
    // SAFETY: plain Win32 calls on our own live window from its owning thread.
    unsafe {
        if GetCursorPos(&mut pt).is_ok() && ScreenToClient(hwnd.get(), &mut pt).as_bool() {
            let lparam = (((pt.y as u32 & 0xFFFF) << 16) | (pt.x as u32 & 0xFFFF)) as isize;
            let _ = PostMessageW(Some(hwnd.get()), WM_LBUTTONUP, WPARAM(0), LPARAM(lparam));
        }
    }
}

struct PidlList(Vec<*mut ITEMIDLIST>);

impl PidlList {
    fn parse(paths: &[PathBuf]) -> Result<Self> {
        let mut list = Self(Vec::with_capacity(paths.len()));
        for path in paths {
            let wide = HSTRING::from(path.as_os_str());
            let mut pidl = std::ptr::null_mut();
            // SAFETY: SHParseDisplayName allocates a PIDL into `pidl`; it is freed in Drop.
            unsafe { SHParseDisplayName(&wide, None::<&IBindCtx>, &mut pidl, 0, None)? };
            list.0.push(pidl);
        }
        Ok(list)
    }
}

impl Drop for PidlList {
    fn drop(&mut self) {
        for pidl in &self.0 {
            // SAFETY: each PIDL came from SHParseDisplayName and is freed exactly once.
            unsafe { ILFree(Some(pidl.cast_const())) };
        }
    }
}

#[derive(Debug, Clone)]
pub enum DropEvent {
    Enter {
        files: usize,
        effect: DROPEFFECT,
        in_zone: bool,
        formats: Vec<String>,
    },
    Over {
        in_zone: bool,
    },
    Leave,
    Drop {
        paths: Vec<PathBuf>,
        effect: DROPEFFECT,
        in_zone: bool,
    },
}

/// Drop zone in logical pixels, refreshed by the view every frame.
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Zone {
    pub fn from_bounds(bounds: Bounds<Pixels>) -> Self {
        Self {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// State shared between the view (writes) and the drop target (reads), UI thread only.
#[derive(Debug)]
pub struct DropShared {
    pub zone: Cell<Option<Zone>>,
    pub scale: Cell<f32>,
}

impl Default for DropShared {
    fn default() -> Self {
        Self {
            zone: Cell::new(None),
            scale: Cell::new(1.0),
        }
    }
}

/// Keeps our registered drop target alive for the window's lifetime.
pub struct OwnDropTarget(#[allow(dead_code)] IDropTarget);

/// Replaces GPUI's drop target (only one may be registered per HWND).
pub fn install(
    hwnd: Hwnd,
    shared: Rc<DropShared>,
    tx: UnboundedSender<DropEvent>,
) -> Result<OwnDropTarget> {
    // SAFETY: COM creation on the UI STA thread.
    let helper: Option<IDropTargetHelper> =
        unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let target: IDropTarget = SpikeDropTarget {
        hwnd: hwnd.get(),
        shared,
        helper,
        tx,
        files: Cell::new(0),
        last: Cell::new((u32::MAX, false)),
    }
    .into();
    // SAFETY: revoke/register on our own live window from its owning (STA) thread.
    unsafe {
        let _ = RevokeDragDrop(hwnd.get());
        RegisterDragDrop(hwnd.get(), &target)?;
    }
    Ok(OwnDropTarget(target))
}

#[implement(IDropTarget)]
struct SpikeDropTarget {
    hwnd: HWND,
    shared: Rc<DropShared>,
    helper: Option<IDropTargetHelper>,
    tx: UnboundedSender<DropEvent>,
    files: Cell<usize>,
    last: Cell<(u32, bool)>,
}

impl SpikeDropTarget {
    fn in_zone(&self, pt: &POINTL) -> bool {
        let mut client = POINT { x: pt.x, y: pt.y };
        // SAFETY: converts a point for our own window.
        if !unsafe { ScreenToClient(self.hwnd, &mut client) }.as_bool() {
            return false;
        }
        let scale = self.shared.scale.get().max(0.5);
        let (x, y) = (client.x as f32 / scale, client.y as f32 / scale);
        self.shared
            .zone
            .get()
            .is_some_and(|zone| zone.contains(x, y))
    }

    /// Effect from modifiers like Explorer: Ctrl copy, Shift move, Alt or Ctrl+Shift link.
    /// Without modifiers the spike picks copy (the real app decides by volume).
    fn effect(&self, keys: MODIFIERKEYS_FLAGS, in_zone: bool) -> DROPEFFECT {
        if self.files.get() == 0 || !in_zone {
            return DROPEFFECT_NONE;
        }
        let ctrl = keys.0 & MK_CONTROL.0 != 0;
        let shift = keys.0 & MK_SHIFT.0 != 0;
        let alt = keys.0 & MK_ALT != 0;
        match (ctrl, shift, alt) {
            (true, true, _) | (_, _, true) => DROPEFFECT_LINK,
            (false, true, false) => DROPEFFECT_MOVE,
            _ => DROPEFFECT_COPY,
        }
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for SpikeDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let data = pdataobj.ok()?;
            let (files, _) = hdrop_paths(data, false);
            self.files.set(files);
            let in_zone = self.in_zone(pt);
            let effect = self.effect(grfkeystate, in_zone);
            // SAFETY: OLE passes a valid out-pointer for the effect.
            unsafe { *pdweffect = effect };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call with our window and the source's data object.
                let _ = unsafe { helper.DragEnter(self.hwnd, data, &point, effect) };
            }
            self.last.set((effect.0, in_zone));
            let _ = self.tx.unbounded_send(DropEvent::Enter {
                files,
                effect,
                in_zone,
                formats: formats(data),
            });
            Ok(())
        })
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let in_zone = self.in_zone(pt);
            let effect = self.effect(grfkeystate, in_zone);
            // SAFETY: OLE passes a valid out-pointer for the effect.
            unsafe { *pdweffect = effect };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call during an active drag.
                let _ = unsafe { helper.DragOver(&point, effect) };
            }
            if self.last.get() != (effect.0, in_zone) {
                self.last.set((effect.0, in_zone));
                let _ = self.tx.unbounded_send(DropEvent::Over { in_zone });
            }
            Ok(())
        })
    }

    fn DragLeave(&self) -> Result<()> {
        guard(Err(Error::from(E_UNEXPECTED)), || {
            if let Some(helper) = &self.helper {
                // SAFETY: helper call during an active drag.
                let _ = unsafe { helper.DragLeave() };
            }
            let _ = self.tx.unbounded_send(DropEvent::Leave);
            Ok(())
        })
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let data = pdataobj.ok()?;
            let in_zone = self.in_zone(pt);
            let effect = self.effect(grfkeystate, in_zone);
            // SAFETY: OLE passes a valid out-pointer for the effect.
            unsafe { *pdweffect = effect };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call with the dropped data object.
                let _ = unsafe { helper.Drop(data, &point, effect) };
            }
            let (_, paths) = hdrop_paths(data, true);
            // The spike only reports drops; it never performs file operations.
            let _ = self.tx.unbounded_send(DropEvent::Drop {
                paths,
                effect,
                in_zone,
            });
            Ok(())
        })
    }
}

/// Number of CF_HDROP files and, when `collect` is set, their paths.
fn hdrop_paths(data: &IDataObject, collect: bool) -> (usize, Vec<PathBuf>) {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut paths = Vec::new();
    // SAFETY: the medium is released before returning; the HDROP is only read while held.
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else {
            return (0, paths);
        };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        if collect {
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
            }
        }
        ReleaseStgMedium(&mut medium);
        (count as usize, paths)
    }
}

/// Names of the formats a drag source offers (e.g. CF_HDROP, FileGroupDescriptorW).
fn formats(data: &IDataObject) -> Vec<String> {
    let mut names = Vec::new();
    // SAFETY: enumerator and buffers are local; strings are copied out.
    unsafe {
        let Ok(enumerator) = data.EnumFormatEtc(DATADIR_GET.0 as u32) else {
            return names;
        };
        let mut item = [FORMATETC::default()];
        while enumerator.Next(&mut item, None).is_ok() && names.len() < 24 {
            let cf = item[0].cfFormat;
            if cf == 0 {
                break;
            }
            let mut buf = [0u16; 128];
            let len = GetClipboardFormatNameW(u32::from(cf), &mut buf);
            names.push(if len > 0 {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                format!("CF#{cf}")
            });
            item[0] = FORMATETC::default();
        }
    }
    names
}
