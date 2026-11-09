//! OLE drag and drop (SYSTEM-INTEGRATION.md §9, ADR-015, ADR-016).
//!
//! - **Out:** [`drag`] carries the shell's own data object for the items, so every target gets
//!   the formats Explorer would give it. It runs a modal loop: call it from the modal host
//!   ([`crate::modal::run`]), never from a GPUI handler or task.
//! - **In:** [`register`] replaces GPUI's drop target (which always answers copy and only gives
//!   paths) with ours. What a drag over the window may do is decided by a [`DropHandler`] the UI
//!   supplies; this module does the OLE side: points, keys, effects the source allows, the
//!   drag image, and what the source is told after the drop.
//!
//! Drags from this app to itself go through the same target, so there is one path for drops.

// The code `#[implement]` generates for the drop target and source trips two pedantic lints
// this crate cannot fix.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{
    COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, E_UNEXPECTED, HWND,
    LPARAM, POINT, POINTL, S_OK, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW, ScreenToClient};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, DVASPECT_CONTENT, FORMATETC, IBindCtx, IDataObject,
    STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, MK_ALT, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{
    MK_CONTROL, MK_LBUTTON, MK_RBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS, SFGAO_CANCOPY, SFGAO_CANLINK,
    SFGAO_CANMOVE, SFGAO_FLAGS,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, CLSID_DragDropHelper, HDROP, IDragSourceHelper, IDropTargetHelper,
    IShellItemImageFactory, SHCreateItemFromIDList, SHCreateShellItemArrayFromIDLists, SHDRAGIMAGE,
    SHDoDragDrop, SIATTRIBFLAGS_AND, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetCursorPos, GetSystemMetrics, IDC_NO, LoadCursorW, PostMessageW,
    SM_CXDRAG, SM_SWAPBUTTON, SetCursor, WM_LBUTTONUP, WM_RBUTTONUP, WindowFromPoint,
};
use windows::core::{BOOL, Error, HRESULT, Ref, w};
use windows_core::implement;

use crate::clipboard::{global, hdrop_paths};
use crate::com::StaGuard;
use crate::error::ShellError;
use crate::menu::Pidl;
use crate::modal::guard;

/// What a drop does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Copy,
    Move,
    /// Make shortcuts.
    Link,
}

impl Effect {
    fn bits(self) -> DROPEFFECT {
        match self {
            Self::None => DROPEFFECT_NONE,
            Self::Copy => DROPEFFECT_COPY,
            Self::Move => DROPEFFECT_MOVE,
            Self::Link => DROPEFFECT_LINK,
        }
    }

    /// The effect a finished drag reports (move first: a target that moved also copied).
    fn from_bits(bits: DROPEFFECT) -> Self {
        if bits.0 & DROPEFFECT_MOVE.0 != 0 {
            Self::Move
        } else if bits.0 & DROPEFFECT_COPY.0 != 0 {
            Self::Copy
        } else if bits.0 & DROPEFFECT_LINK.0 != 0 {
            Self::Link
        } else {
            Self::None
        }
    }
}

/// The effects a drag source allows.
// One bool per OLE effect: they combine freely.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allowed {
    pub copy: bool,
    pub moves: bool,
    pub link: bool,
}

impl Allowed {
    pub const ALL: Self = Self {
        copy: true,
        moves: true,
        link: true,
    };

    pub fn allows(self, effect: Effect) -> bool {
        match effect {
            Effect::None => true,
            Effect::Copy => self.copy,
            Effect::Move => self.moves,
            Effect::Link => self.link,
        }
    }

    fn bits(self) -> DROPEFFECT {
        let mut bits = 0;
        if self.copy {
            bits |= DROPEFFECT_COPY.0;
        }
        if self.moves {
            bits |= DROPEFFECT_MOVE.0;
        }
        if self.link {
            bits |= DROPEFFECT_LINK.0;
        }
        DROPEFFECT(bits)
    }

    fn from_bits(bits: DROPEFFECT) -> Self {
        Self {
            copy: bits.0 & DROPEFFECT_COPY.0 != 0,
            moves: bits.0 & DROPEFFECT_MOVE.0 != 0,
            link: bits.0 & DROPEFFECT_LINK.0 != 0,
        }
    }
}

/// Modifier keys during a drag, and whether it is a right-button drag.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub right: bool,
}

impl Keys {
    fn from_state(state: MODIFIERKEYS_FLAGS) -> Self {
        Self {
            ctrl: state.0 & MK_CONTROL.0 != 0,
            shift: state.0 & MK_SHIFT.0 != 0,
            alt: state.0 & MK_ALT != 0,
            right: state.0 & MK_RBUTTON.0 != 0,
        }
    }
}

/// A point in the target window's client area, in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientPoint {
    pub x: i32,
    pub y: i32,
}

/// What the source is told after a drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReply {
    /// Not taken.
    Refused,
    /// The items are being copied here; the source keeps them.
    Copy,
    /// The items are being moved here, by this app (an *optimized move*): the source must not
    /// delete anything, so it is told that nothing is left for it to do.
    Move,
    /// What happens is decided afterwards (a menu or a confirmation), by this app: as for
    /// `Move`, the source must not act on it.
    Later,
}

/// Decides what a drag over the window does. Called on the UI thread from OLE callbacks, which
/// can come inside another modal loop: it must answer at once and must not re-enter GPUI.
/// Every effect it returns is limited to what the source allows before OLE sees it.
pub trait DropHandler {
    /// A drag came in. `paths` are its file-system items (empty when it carries none).
    fn enter(&self, paths: Vec<PathBuf>, at: ClientPoint, keys: Keys, allowed: Allowed) -> Effect;
    fn over(&self, at: ClientPoint, keys: Keys, allowed: Allowed) -> Effect;
    /// The drag left the window or was cancelled.
    fn leave(&self);
    /// The items were dropped. The data object is only valid during the call, so `paths` are
    /// read before it.
    fn dropped(
        &self,
        paths: Vec<PathBuf>,
        at: ClientPoint,
        keys: Keys,
        allowed: Allowed,
    ) -> DropReply;
}

/// Our drop target, registered on a window in place of GPUI's; revoked when dropped.
pub struct DropRegistration {
    hwnd: HWND,
    _target: IDropTarget,
}

impl std::fmt::Debug for DropRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropRegistration").finish_non_exhaustive()
    }
}

impl Drop for DropRegistration {
    fn drop(&mut self) {
        // SAFETY: revokes our own registration on the window, on the thread that made it. A
        // window already destroyed has nothing registered; that error is of no interest.
        let _ = unsafe { RevokeDragDrop(self.hwnd) };
    }
}

/// Puts our drop target on `owner` (only one can be registered per window, so GPUI's goes).
/// Call on the UI thread, which OLE already initialised (GPUI calls `OleInitialize`).
pub fn register(
    owner: isize,
    handler: Rc<dyn DropHandler>,
) -> Result<DropRegistration, ShellError> {
    let hwnd = HWND(owner as *mut core::ffi::c_void);
    // SAFETY: COM creation on the UI (STA) thread; the helper only draws the drag image, so
    // drops still work without it.
    let helper: Option<IDropTargetHelper> =
        unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let target: IDropTarget = Target {
        hwnd,
        handler,
        helper,
        right: Cell::new(false),
    }
    .into();
    // SAFETY: revoke and register on our own live window, from its owning (STA) thread.
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        RegisterDragDrop(hwnd, &target).map_err(ShellError::Com)?;
    }
    Ok(DropRegistration {
        hwnd,
        _target: target,
    })
}

/// Not agile: it holds the UI's `Rc` handler, so COM calls it on the STA that registered it.
#[implement(IDropTarget, Agile = false)]
struct Target {
    hwnd: HWND,
    handler: Rc<dyn DropHandler>,
    helper: Option<IDropTargetHelper>,
    /// A right-button drag: the button is up by the time of `Drop`, so it is remembered.
    right: Cell<bool>,
}

/// Reads the source's allowed effects and sets the answer to NONE before anything can fail.
///
/// # Safety
/// `pdweffect` must be the in/out pointer OLE passes to `IDropTarget` methods.
unsafe fn take_allowed(pdweffect: *mut DROPEFFECT) -> DROPEFFECT {
    if pdweffect.is_null() {
        return DROPEFFECT_NONE;
    }
    // SAFETY: guaranteed by the caller.
    unsafe {
        let allowed = *pdweffect;
        *pdweffect = DROPEFFECT_NONE;
        allowed
    }
}

/// Writes the answer, never more than the source allows.
///
/// # Safety
/// As for [`take_allowed`].
unsafe fn answer(pdweffect: *mut DROPEFFECT, allowed: DROPEFFECT, effect: Effect) -> DROPEFFECT {
    let bits = if Allowed::from_bits(allowed).allows(effect) {
        effect.bits()
    } else {
        DROPEFFECT_NONE
    };
    if !pdweffect.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { *pdweffect = bits };
    }
    bits
}

impl Target {
    fn client(&self, pt: &POINTL) -> ClientPoint {
        let mut point = POINT { x: pt.x, y: pt.y };
        // SAFETY: converts a point for our own window. On failure the screen point is kept,
        // which lands outside every drop zone.
        let _ = unsafe { ScreenToClient(self.hwnd, &raw mut point) };
        ClientPoint {
            x: point.x,
            y: point.y,
        }
    }

    fn keys(&self, state: MODIFIERKEYS_FLAGS) -> Keys {
        let mut keys = Keys::from_state(state);
        if state.0 & (MK_LBUTTON.0 | MK_RBUTTON.0) != 0 {
            self.right.set(keys.right);
        }
        keys.right = self.right.get();
        keys
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: OLE passes a valid in/out effect pointer.
        let allowed = unsafe { take_allowed(pdweffect) };
        self.right.set(false);
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let data = pdataobj.ok()?;
            let keys = self.keys(grfkeystate);
            let effect = self.handler.enter(
                file_paths(data),
                self.client(pt),
                keys,
                Allowed::from_bits(allowed),
            );
            // SAFETY: OLE passes a valid in/out effect pointer.
            let bits = unsafe { answer(pdweffect, allowed, effect) };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call with our window and the source's data object.
                let _ = unsafe { helper.DragEnter(self.hwnd, data, &raw const point, bits) };
            }
            Ok(())
        })
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: OLE passes a valid in/out effect pointer.
        let allowed = unsafe { take_allowed(pdweffect) };
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let keys = self.keys(grfkeystate);
            let effect = self
                .handler
                .over(self.client(pt), keys, Allowed::from_bits(allowed));
            // SAFETY: OLE passes a valid in/out effect pointer.
            let bits = unsafe { answer(pdweffect, allowed, effect) };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call during an active drag.
                let _ = unsafe { helper.DragOver(&raw const point, bits) };
            }
            Ok(())
        })
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        guard(Err(Error::from(E_UNEXPECTED)), || {
            if let Some(helper) = &self.helper {
                // SAFETY: helper call during an active drag.
                let _ = unsafe { helper.DragLeave() };
            }
            self.handler.leave();
            Ok(())
        })
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: OLE passes a valid in/out effect pointer.
        let allowed = unsafe { take_allowed(pdweffect) };
        guard(Err(Error::from(E_UNEXPECTED)), || {
            let data = pdataobj.ok()?;
            let keys = self.keys(grfkeystate);
            let reply = self.handler.dropped(
                file_paths(data),
                self.client(pt),
                keys,
                Allowed::from_bits(allowed),
            );
            // Only a copy is reported as done. A move is ours from start to end (the source is
            // told nothing is left to do, so it never deletes originals we have not moved yet);
            // so is anything decided later.
            let (shown, reported) = match reply {
                DropReply::Copy => (Effect::Copy, Effect::Copy),
                DropReply::Move => {
                    tell_source(data, Effect::None, Effect::Move);
                    (Effect::Move, Effect::None)
                }
                DropReply::Later | DropReply::Refused => (Effect::None, Effect::None),
            };
            // SAFETY: OLE passes a valid in/out effect pointer.
            unsafe { answer(pdweffect, allowed, reported) };
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                // SAFETY: helper call with the dropped data object.
                let _ = unsafe { helper.Drop(data, &raw const point, shown.bits()) };
            }
            Ok(())
        })
    }
}

/// The file-system paths a data object carries (`CF_HDROP`); empty when it has none.
fn file_paths(data: &IDataObject) -> Vec<PathBuf> {
    let format = hglobal_format(CF_HDROP.0);
    // SAFETY: the medium is released before returning, and the HDROP is only read while held,
    // after checking that the source really returned an HGLOBAL.
    unsafe {
        if data.QueryGetData(&raw const format) != S_OK {
            return Vec::new();
        }
        let Ok(mut medium) = data.GetData(&raw const format) else {
            return Vec::new();
        };
        let paths = if medium.tymed == TYMED_HGLOBAL.0 as u32 && !medium.u.hGlobal.is_invalid() {
            hdrop_paths(HDROP(medium.u.hGlobal.0))
        } else {
            Vec::new()
        };
        ReleaseStgMedium(&raw mut medium);
        usable(paths)
    }
}

/// The paths a drop may act on. A relative path would be read against our working folder; an
/// item inside a Recycle Bin (`$R…`) moved away would leave its `$I` record behind. Either one
/// refuses the whole drop: nothing is taken in part.
fn usable(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let bin = |path: &PathBuf| {
        path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("$recycle.bin")
        })
    };
    if paths.iter().any(|path| !path.is_absolute() || bin(path)) {
        return Vec::new();
    }
    paths
}

fn hglobal_format(format: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Tells the source what was performed ("Handling Shell Data Transfer Scenarios": an optimized
/// move reports NONE performed and MOVE as the logical effect). A source that does not take the
/// formats simply goes by the effect `Drop` returned.
fn tell_source(data: &IDataObject, performed: Effect, logical: Effect) {
    // SAFETY: registers (or looks up) named formats with static names.
    let (performed_format, logical_format) = unsafe {
        (
            RegisterClipboardFormatW(w!("Performed DropEffect")),
            RegisterClipboardFormatW(w!("Logical Performed DropEffect")),
        )
    };
    for (format, effect) in [(performed_format, performed), (logical_format, logical)] {
        let Ok(format) = u16::try_from(format) else {
            continue;
        };
        let bits = effect.bits().0;
        let Ok(block) = global(4, |bytes| bytes.copy_from_slice(&bits.to_le_bytes())) else {
            continue;
        };
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: block.0 },
            pUnkForRelease: ManuallyDrop::new(None),
        };
        let format = hglobal_format(format);
        // SAFETY: with `fRelease` the data object owns the block once SetData succeeds, so our
        // guard is forgotten only then; on failure the guard frees it.
        if unsafe { data.SetData(&raw const format, &raw const medium, true) }.is_ok() {
            std::mem::forget(block);
        }
    }
}

/// The items of a drag, parsed by [`prepare`]: that can take long (many items, a network
/// folder), so it runs off the UI thread before the drag starts.
#[derive(Debug)]
pub struct PreparedDrag {
    pidls: Vec<Pidl>,
}

/// Parses the items of a drag. Any thread: it enters an STA for the call.
pub fn prepare(paths: &[PathBuf]) -> Result<PreparedDrag, ShellError> {
    let _com = StaGuard::new()?;
    if paths.is_empty() {
        return Err(ShellError::InvalidPath(String::new()));
    }
    let pidls = paths
        .iter()
        .map(|path| Pidl::parse(path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedDrag { pidls })
}

/// A drag of files out of a window.
#[derive(Debug)]
pub struct DragOut {
    pub owner: isize,
    pub items: PreparedDrag,
    /// What the drag offers; the items' own abilities (`SFGAO_CAN*`) narrow it further.
    pub allowed: Allowed,
    /// Whether the items may be dropped outside the owner window (`dnd.external_out`). When not,
    /// the pointer shows "no" over other windows and a release there cancels the drag.
    pub leave_window: bool,
    /// Started with the right button.
    pub right: bool,
}

/// Runs the drag and returns what the target did ([`Effect::None`] when cancelled). A modal
/// loop: run it from the modal host on the UI thread (ADR-015).
pub fn drag(request: &DragOut) -> Result<Effect, ShellError> {
    let pidls = &request.items.pidls;
    let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|pidl| pidl.0.cast_const()).collect();
    let owner = HWND(request.owner as *mut core::ffi::c_void);
    // The items were prepared off the UI thread: the button may be up by now, and a drag
    // started then would drop at once wherever the pointer is.
    if !button_held(request.right) {
        return Ok(Effect::None);
    }
    let source: IDropSource = Source {
        owner,
        leave_window: request.leave_window,
        button: if request.right {
            MK_RBUTTON
        } else {
            MK_LBUTTON
        },
    }
    .into();
    // SAFETY: the PIDLs are owned by `pidls` and outlive the calls; the COM objects are made and
    // used on this (UI, STA) thread.
    let effect = unsafe {
        let items = SHCreateShellItemArrayFromIDLists(&list).map_err(ShellError::Com)?;
        // What the items themselves allow (a drive cannot be moved; read-only media cannot be
        // moved), as Explorer's own views do.
        let abilities = items
            .GetAttributes(
                SIATTRIBFLAGS_AND,
                SFGAO_FLAGS(SFGAO_CANCOPY.0 | SFGAO_CANMOVE.0 | SFGAO_CANLINK.0),
            )
            .map_err(ShellError::Com)?;
        let allowed = Allowed {
            copy: request.allowed.copy && abilities.0 & SFGAO_CANCOPY.0 != 0,
            moves: request.allowed.moves && abilities.0 & SFGAO_CANMOVE.0 != 0,
            link: request.allowed.link && abilities.0 & SFGAO_CANLINK.0 != 0,
        };
        if allowed == Allowed::default() {
            return Ok(Effect::None);
        }
        let data: IDataObject = items
            .BindToHandler(None::<&IBindCtx>, &BHID_DataObject)
            .map_err(ShellError::Com)?;
        if let Some(first) = pidls.first() {
            set_drag_image(owner, first, &data);
        }
        OUR_DRAG.with(|ours| ours.set(true));
        let done = SHDoDragDrop(Some(owner), &data, &source, allowed.bits());
        OUR_DRAG.with(|ours| ours.set(false));
        done
    };
    // Also after a failure: the loop may have swallowed the button-up.
    release_button(request.owner, request.right);
    // A cancel is a success code (DRAGDROP_S_CANCEL), with no effect.
    Ok(Effect::from_bits(effect.map_err(ShellError::Com)?))
}

thread_local! {
    /// A drag from one of our windows is running (on this, the UI, thread): a drop of it on
    /// another of our windows comes from inside the app.
    static OUR_DRAG: Cell<bool> = const { Cell::new(false) };
}

/// Whether the drag over a window is one of ours (see [`drag`]).
pub fn dragging_out() -> bool {
    OUR_DRAG.with(Cell::get)
}

/// Whether the left (or right) button is down now, as the user sees it: `GetAsyncKeyState`
/// reads the physical buttons, which are swapped for a left-handed mouse.
fn button_held(right: bool) -> bool {
    // SAFETY: plain state queries.
    unsafe {
        let swapped = GetSystemMetrics(SM_SWAPBUTTON) != 0;
        // Swapped buttons: the right one is physically on the left, and the other way round.
        let key = if right ^ swapped {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(key.0)).cast_unsigned() & 0x8000 != 0
    }
}

/// The drag image: the first item's icon, with the pointer near its bottom. Without one the
/// shell asks the window for an image (`DI_GETDRAGIMAGE`), which GPUI does not answer, and shows
/// a blank box. Best effort: on failure the shell's default stands.
fn set_drag_image(owner: HWND, item: &Pidl, data: &IDataObject) {
    // SAFETY: COM calls on this (UI, STA) thread with a live PIDL and data object. The bitmap
    // belongs to the drag-image manager once InitializeFromBitmap succeeds; otherwise it is
    // deleted here.
    unsafe {
        let Ok(helper) = CoCreateInstance::<_, IDragSourceHelper>(
            &CLSID_DragDropHelper,
            None,
            CLSCTX_INPROC_SERVER,
        ) else {
            return;
        };
        let Ok(factory) = SHCreateItemFromIDList::<IShellItemImageFactory>(item.0.cast_const())
        else {
            return;
        };
        // Icons only: a thumbnail can take long to make (a video, a network file), and this
        // runs on the UI thread.
        let dpi = GetDpiForWindow(owner).max(96);
        let edge = i32::try_from(DRAG_IMAGE_PX * dpi / 96).unwrap_or(96);
        let size = SIZE { cx: edge, cy: edge };
        let Ok(bitmap) = factory.GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK) else {
            return;
        };
        // The bitmap may be bigger than asked for: the image says its real size.
        let mut info = BITMAP::default();
        let read = GetObjectW(
            bitmap.into(),
            i32::try_from(size_of::<BITMAP>()).unwrap_or(0),
            Some((&raw mut info).cast()),
        );
        let (width, height) = if read > 0 && info.bmWidth > 0 && info.bmHeight != 0 {
            (info.bmWidth, info.bmHeight.abs())
        } else {
            (edge, edge)
        };
        let image = SHDRAGIMAGE {
            sizeDragImage: SIZE {
                cx: width,
                cy: height,
            },
            ptOffset: POINT {
                x: width / 2,
                y: height - height / 8,
            },
            hbmpDragImage: bitmap,
            // CLR_NONE: the bitmap's alpha says what is transparent.
            crColorKey: COLORREF(0xFFFF_FFFF),
        };
        if helper.InitializeFromBitmap(&raw const image, data).is_err() {
            let _ = DeleteObject(bitmap.into());
        }
    }
}

/// The side of the drag image, in pixels at 100% scale (Explorer's large icon).
const DRAG_IMAGE_PX: u32 = 96;

/// The modal loop swallows the button-up, so the window would still see the button held:
/// post it, unless the button really is still down (Escape cancelled the drag).
fn release_button(owner: isize, right: bool) {
    let hwnd = HWND(owner as *mut core::ffi::c_void);
    let message = if right { WM_RBUTTONUP } else { WM_LBUTTONUP };
    let mut point = POINT::default();
    // SAFETY: plain Win32 calls on our own live window from its owning thread.
    unsafe {
        if button_held(right)
            || GetCursorPos(&raw mut point).is_err()
            || !ScreenToClient(hwnd, &raw mut point).as_bool()
        {
            return;
        }
        let packed =
            ((point.y.cast_unsigned() & 0xFFFF) << 16) | (point.x.cast_unsigned() & 0xFFFF);
        let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(packed as isize));
    }
}

/// Not agile: made and used on the UI thread for one drag.
#[implement(IDropSource, Agile = false)]
struct Source {
    owner: HWND,
    leave_window: bool,
    /// `MK_LBUTTON` or `MK_RBUTTON`: the button that started the drag.
    button: MODIFIERKEYS_FLAGS,
}

impl Source {
    /// Over another app while drops there are not allowed.
    fn blocked_here(&self) -> bool {
        if self.leave_window {
            return false;
        }
        let mut point = POINT::default();
        // SAFETY: plain queries about the window under the pointer.
        unsafe {
            if GetCursorPos(&raw mut point).is_err() {
                return false;
            }
            let under = WindowFromPoint(point);
            under.is_invalid() || GetAncestor(under, GA_ROOT) != self.owner
        }
    }
}

#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        guard(DRAGDROP_S_CANCEL, || {
            if fescapepressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            let other = if self.button == MK_LBUTTON {
                MK_RBUTTON
            } else {
                MK_LBUTTON
            };
            // The other button too: cancelled, as in Explorer.
            if grfkeystate.0 & other.0 != 0 {
                return DRAGDROP_S_CANCEL;
            }
            if grfkeystate.0 & self.button.0 == 0 {
                return if self.blocked_here() {
                    DRAGDROP_S_CANCEL
                } else {
                    DRAGDROP_S_DROP
                };
            }
            S_OK
        })
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        guard(DRAGDROP_S_USEDEFAULTCURSORS, || {
            if !self.blocked_here() {
                return DRAGDROP_S_USEDEFAULTCURSORS;
            }
            // SAFETY: loads a shared system cursor (never freed) and shows it.
            unsafe {
                if let Ok(cursor) = LoadCursorW(None, IDC_NO) {
                    SetCursor(Some(cursor));
                }
            }
            S_OK
        })
    }
}

/// How far the pointer moves before a press becomes a drag, in pixels at 100% scale
/// (`SM_CXDRAG`); the Drag Guard's default threshold.
pub fn system_threshold() -> f32 {
    // SAFETY: a plain metric query.
    let px = unsafe { GetSystemMetricsForDpi(SM_CXDRAG, 96) };
    if px > 0 { px as f32 } else { 4.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_effects_round_trip() {
        for allowed in [
            Allowed::ALL,
            Allowed::default(),
            Allowed {
                copy: true,
                moves: false,
                link: true,
            },
        ] {
            assert_eq!(Allowed::from_bits(allowed.bits()), allowed);
        }
        assert!(Allowed::default().allows(Effect::None));
        assert!(!Allowed::default().allows(Effect::Move));
    }

    #[test]
    fn a_finished_drag_reports_move_before_copy() {
        let both = DROPEFFECT(DROPEFFECT_MOVE.0 | DROPEFFECT_COPY.0);
        assert_eq!(Effect::from_bits(both), Effect::Move);
        assert_eq!(Effect::from_bits(DROPEFFECT_NONE), Effect::None);
    }

    #[test]
    fn an_answer_never_exceeds_what_the_source_allows() {
        let mut out = DROPEFFECT_NONE;
        // SAFETY: a valid local in/out pointer.
        let bits = unsafe { answer(&raw mut out, DROPEFFECT_COPY, Effect::Move) };
        assert_eq!(bits, DROPEFFECT_NONE);
        assert_eq!(out, DROPEFFECT_NONE);
        // SAFETY: as above.
        let bits = unsafe { answer(&raw mut out, DROPEFFECT_COPY, Effect::Copy) };
        assert_eq!(bits, DROPEFFECT_COPY);
    }

    #[test]
    fn drops_take_absolute_paths_outside_the_recycle_bin_only() {
        let fine = vec![
            PathBuf::from(r"C:\Data\a.txt"),
            PathBuf::from(r"\\server\share\b"),
        ];
        assert_eq!(usable(fine.clone()), fine);
        let none: Vec<PathBuf> = Vec::new();
        let relative = vec![PathBuf::from(r"C:\Data\a.txt"), PathBuf::from("b.txt")];
        assert_eq!(usable(relative), none);
        let binned = vec![PathBuf::from(r"C:\$Recycle.Bin\S-1-5\$RABC.txt")];
        assert_eq!(usable(binned), none);
    }

    #[test]
    fn keys_come_from_the_key_state() {
        let keys = Keys::from_state(MODIFIERKEYS_FLAGS(MK_CONTROL.0 | MK_RBUTTON.0 | MK_ALT));
        assert!(keys.ctrl && keys.right && keys.alt && !keys.shift);
    }
}
