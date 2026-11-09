//! Drag and drop (DEVELOPER-MODE.md §5, SYSTEM-INTEGRATION.md §9, DESIGN.md §9.3).
//!
//! - A press on a list row or a sidebar item goes through the Drag Guard ([`DragGuard`]); when
//!   it becomes a drag, the items go out through OLE (`sx_shell::dnd::drag`, on the modal host).
//! - Drops, from this app or another, arrive at our OLE drop target. The [`Router`] answers it
//!   at once from what the last frame drew (the list's rows, the content pane, the sidebar's
//!   items) and the switches; the drop itself is run here, by `sx-ops`, with journal and undo.

use std::cell::{Cell, RefCell};
use std::path::{Component, Path, PathBuf, Prefix};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{
    Bounds, Context, DispatchPhase, Div, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollEvent, Styled as _, Window, canvas, div, point, px,
};
use sx_core::Location;
use sx_core::drag::{
    Blocked, Button, DefaultAction, DragGuard, DragOrigin, DragPolicy, DropEffect, DropTarget,
    Modifier, Modifiers, Step,
};
use sx_fs::DirSnapshot;
use sx_ops::Mode;
use sx_settings::Features;
use sx_shell::dnd::{Allowed, ClientPoint, DragOut, DropHandler, DropReply, Effect, Keys};
use sx_theme::Token;

use crate::sidebar::SidebarTarget;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// `dnd.blocked_hint` says the same thing at most this often (DESIGN.md §9.3).
const HINT_EVERY: Duration = Duration::from_secs(30);

/// The drag switches as the Drag Guard reads them.
pub(crate) fn policy(features: &Features, system_threshold: f32) -> DragPolicy {
    DragPolicy {
        enabled: features.dnd_enabled,
        source_files: features.dnd_source_files,
        source_folders: features.dnd_source_folders,
        source_sidebar: features.dnd_source_sidebar,
        external_out: features.dnd_external_out,
        external_in: features.dnd_external_in,
        target_sidebar: features.dnd_target_sidebar,
        target_folders: features.dnd_target_folders,
        readonly: features.safety_readonly_mode,
        threshold_px: f32::from(features.dnd_threshold_px),
        system_threshold_px: system_threshold,
        hold_delay_ms: u64::from(features.dnd_hold_delay_ms),
        scroll_cooldown_ms: u64::from(features.dnd_scroll_cooldown_ms),
        require_modifier: match features.dnd_require_modifier {
            "alt" => Some(Modifier::Alt),
            "ctrl" => Some(Modifier::Ctrl),
            "shift" => Some(Modifier::Shift),
            _ => None,
        },
        right_drag_bypass: features.dnd_right_drag_bypass,
        default_action: match features.dnd_default_action {
            "copy" => DefaultAction::Copy,
            "move" => DefaultAction::Move,
            "ask" => DefaultAction::Ask,
            _ => DefaultAction::System,
        },
        confirm_folder_move: features.dnd_confirm_folder_move,
        pins_locked: features.sidebar_pins_locked,
    }
}

/// Where a drop would land, for the highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// A folder row of a tab's list, by entry index.
    Row { tab: u64, index: u32 },
    /// The open folder itself: the content pane outside folder rows.
    Folder { tab: u64 },
    /// A sidebar item.
    Sidebar(Location),
}

/// A drop's place, the folder it goes into, and which switch governs it.
#[derive(Clone, Debug)]
struct Spot {
    place: Place,
    dest: PathBuf,
    target: DropTarget,
}

/// The active tab's list as the drop target sees it, refreshed when the tab or its listing
/// changes.
pub(crate) struct ListZone {
    pub tab: u64,
    /// The rows area (folder rows are hit-tested there) and how far it is scrolled, both
    /// written by the rows canvas every frame.
    pub rows: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub offset: Rc<Cell<Pixels>>,
    pub row_height: Pixels,
    pub snapshot: Arc<DirSnapshot>,
    /// The open folder; `None` for search results (only their folder rows take drops).
    pub folder: Option<PathBuf>,
}

/// The drag our own window started, while it runs.
#[derive(Clone, Copy, Debug)]
struct Outgoing {
    /// It started in the sidebar: dropping it on a sidebar item would move a pinned folder,
    /// which reordering pins never does (DEVELOPER-MODE.md §5.3).
    from_sidebar: bool,
    /// The switches when its button went down: they hold until it ends (§5.4).
    policy: DragPolicy,
}

/// The items of the drag over the window, with what every pointer move needs worked out once.
#[derive(Default)]
struct Incoming {
    paths: Vec<PathBuf>,
    /// The paths in comparable form ([`key`]).
    keys: Vec<String>,
    /// The folder they are all in, when they share one.
    parent: Option<String>,
    /// A drive or a share itself is among them: only a shortcut could be made of it.
    roots: bool,
    volume: Option<String>,
    /// The last destination asked about: (its key, the items are in it or are it, they are
    /// already there).
    last: Option<(String, bool, bool)>,
}

impl Incoming {
    fn new(paths: Vec<PathBuf>) -> Self {
        let keys: Vec<String> = paths.iter().map(|path| key(path)).collect();
        let mut parents = paths.iter().map(|path| path.parent().map(key));
        let first = parents.next().flatten();
        let parent = first.filter(|first| parents.all(|p| p.as_ref() == Some(first)));
        let volume = paths.first().and_then(|path| volume(path));
        let roots = paths.iter().any(|path| path.parent().is_none());
        Self {
            paths,
            keys,
            parent,
            roots,
            volume,
            last: None,
        }
    }

    /// Whether `dest` is one of the items or inside one, and whether all items are already in
    /// `dest`. Remembered for the last destination: a drag asks many times about the same one.
    fn relation(&mut self, dest: &Path) -> (bool, bool) {
        let dest = key(dest);
        if let Some((last, inside, here)) = &self.last
            && *last == dest
        {
            return (*inside, *here);
        }
        let inside = self.keys.iter().any(|source| within(&dest, source));
        let here = self.parent.as_deref() == Some(dest.as_str());
        self.last = Some((dest, inside, here));
        (inside, here)
    }
}

/// What the drop target makes of the pointer: the highlight, what the source is shown, what a
/// drop there would do, and the switch that refuses it (for the hint).
struct Decision {
    hover: Option<(Place, bool)>,
    shown: Effect,
    drop: Option<(Spot, DropEffect)>,
    refused_by: Option<&'static str>,
    /// A move may still be stopped by its confirmation (`dnd.confirm_folder_move`).
    confirm_moves: bool,
}

impl Decision {
    fn nothing() -> Self {
        Self {
            hover: None,
            shown: Effect::None,
            drop: None,
            refused_by: None,
            confirm_moves: false,
        }
    }

    fn refused(place: Place, flag: &'static str) -> Self {
        Self {
            hover: Some((place, true)),
            refused_by: Some(flag),
            ..Self::nothing()
        }
    }
}

/// What the drop target tells the window.
#[derive(Debug)]
pub(crate) enum DropEvent {
    /// The drag is over this place (`true`: a switch refuses it there), or nowhere that takes it.
    Hover(Option<(Place, bool)>),
    /// A switch refuses the drag where it is (for the hint).
    Refused(&'static str),
    Drop {
        paths: Vec<PathBuf>,
        dest: PathBuf,
        action: DropAction,
        /// What the source allows (the menu of `Ask` offers only that).
        allowed: Allowed,
        /// Where it was dropped, in window coordinates (the menu of `Ask` opens there).
        at: Point<Pixels>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropAction {
    Copy,
    Move,
    /// Show the menu: copy here, move here, cancel.
    Ask,
}

/// What the drop target reads during a drag; written by the window. UI thread only.
pub(crate) struct Shared {
    pub policy: Cell<DragPolicy>,
    pub scale: Cell<f32>,
    pub list: RefCell<Option<ListZone>>,
    /// The content pane this frame (cleared before each frame; set when it is drawn).
    pub pane: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub sidebar: Rc<RefCell<Vec<SidebarTarget>>>,
    /// A dialog, the palette, a menu or a rename is open: nothing is dropped behind it.
    pub busy: Cell<bool>,
    outgoing: Cell<Option<Outgoing>>,
    incoming: RefCell<Incoming>,
    hover: RefCell<Option<(Place, bool)>>,
    refused: Cell<Option<&'static str>>,
    events: UnboundedSender<DropEvent>,
}

/// A press that may become a drag: what would be dragged, and how it started.
#[derive(Clone, Debug)]
enum Pressed {
    /// A list row: the selection when the row is in it, else the row.
    Row(usize),
    /// A sidebar item's folder.
    Sidebar(PathBuf),
}

#[derive(Clone, Debug)]
struct Press {
    what: Pressed,
    button: Button,
    policy: DragPolicy,
    at: (f32, f32),
    /// `dnd.require_modifier` asks for a key that was not held: a drag cannot start.
    missing_key: Option<Modifier>,
    /// The drag would carry both folders and files (a refusal stops all of it).
    mixed: bool,
}

/// Drag and drop state of a window.
pub(crate) struct DragState {
    pub shared: Rc<Shared>,
    events: Option<UnboundedReceiver<DropEvent>>,
    registration: Option<sx_shell::dnd::DropRegistration>,
    guard: DragGuard,
    press: Option<Press>,
    /// A button is down on something draggable: mouse moves go to the guard.
    pressing: Rc<Cell<bool>>,
    /// When anything in the window last scrolled (`dnd.scroll_cooldown_ms`).
    last_scroll: Rc<Cell<Option<Instant>>>,
    epoch: Instant,
    system_threshold: f32,
    /// Where a drag over the window would land, for the highlight.
    pub hover: Option<(Place, bool)>,
    /// A drag started from the press under way: its button-up is not a click.
    pub dragged: bool,
    /// Hints shown lately, by switch (`dnd.blocked_hint`: once per 30 s each).
    hinted: Vec<(&'static str, Instant)>,
}

impl std::fmt::Debug for DragState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragState")
            .field("hover", &self.hover)
            .field("dragged", &self.dragged)
            .finish_non_exhaustive()
    }
}

impl DragState {
    pub(crate) fn new(sidebar: Rc<RefCell<Vec<SidebarTarget>>>) -> Self {
        let (tx, rx) = unbounded();
        let system_threshold = sx_shell::dnd::system_threshold();
        Self {
            shared: Rc::new(Shared {
                policy: Cell::new(DragPolicy::default()),
                scale: Cell::new(1.0),
                list: RefCell::new(None),
                pane: Rc::new(Cell::new(None)),
                sidebar,
                busy: Cell::new(false),
                outgoing: Cell::new(None),
                incoming: RefCell::new(Incoming::default()),
                hover: RefCell::new(None),
                refused: Cell::new(None),
                events: tx,
            }),
            events: Some(rx),
            registration: None,
            guard: DragGuard::default(),
            press: None,
            pressing: Rc::new(Cell::new(false)),
            last_scroll: Rc::new(Cell::new(None)),
            epoch: Instant::now(),
            system_threshold,
            hover: None,
            dragged: false,
            hinted: Vec::new(),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn ms_of(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
    }

    fn end_press(&mut self) {
        self.press = None;
        self.pressing.set(false);
        self.guard.cancel();
    }

    /// Whether a hint about `flag` may show now; records it when it may.
    fn may_hint(&mut self, flag: &'static str) -> bool {
        let now = Instant::now();
        self.hinted
            .retain(|(_, at)| now.saturating_duration_since(*at) < HINT_EVERY);
        if self.hinted.iter().any(|(shown, _)| *shown == flag) {
            return false;
        }
        self.hinted.push((flag, now));
        true
    }
}

impl Workspace {
    /// Puts our drop target on the window and starts handling what it reports.
    pub(crate) fn install_drop_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.hwnd else {
            return;
        };
        let router: Rc<dyn DropHandler> = Rc::new(Router(self.drag.shared.clone()));
        match sx_shell::dnd::register(owner, router) {
            Ok(registration) => self.drag.registration = Some(registration),
            Err(err) => {
                tracing::warn!(error = %err, "drop target unavailable");
                self.notice = Some(rust_i18n::t!("dnd.no_drop_target").into());
            }
        }
        let Some(mut events) = self.drag.events.take() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.on_drop_event(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// What the drop target needs from this frame: the switches, the scale, what is open over
    /// the window, and the list (rebuilt only when the tab or its listing changed).
    pub(crate) fn sync_drag(&self, window: &Window, cx: &gpui_kit::App) {
        let shared = &self.drag.shared;
        shared
            .policy
            .set(policy(&self.features, self.drag.system_threshold));
        shared.scale.set(window.scale_factor());
        shared.busy.set(
            self.ops.dialog.is_some()
                || self.ops.renaming.is_some()
                || self.palette.is_some()
                || self.context_menu.is_some(),
        );
        // Set again when the pane is drawn this frame.
        shared.pane.set(None);
        let tab = self.tab();
        let Ok(mut list) = shared.list.try_borrow_mut() else {
            return;
        };
        let current = match (list.as_ref(), tab.snapshot()) {
            (Some(zone), Some(snapshot)) => {
                zone.tab == tab.id && Arc::ptr_eq(&zone.snapshot, snapshot)
            }
            (None, None) => true,
            _ => false,
        };
        if current {
            return;
        }
        *list = tab.snapshot().map(|snapshot| ListZone {
            tab: tab.id,
            rows: tab.rows_bounds.clone(),
            offset: tab.rows_offset.clone(),
            row_height: cx.tokens().px(Token::RowHeightDefault),
            snapshot: snapshot.clone(),
            folder: (!snapshot.is_results())
                .then(|| tab.location().as_path().map(Path::to_path_buf))
                .flatten(),
        });
    }

    /// Records the content pane's bounds for the drop target (drawn in every state: rows,
    /// empty, no match, loading).
    pub(crate) fn pane_bounds(&self) -> impl IntoElement {
        let pane = self.drag.shared.pane.clone();
        canvas(move |area, _, _| pane.set(Some(area)), |_, (), _, _| {})
            .absolute()
            .inset_0()
    }

    /// Window-wide listeners: moves and releases while a button is down on something draggable
    /// (they go on outside the item), every scroll (`dnd.scroll_cooldown_ms`), and right-button
    /// presses (a new right-click is never swallowed by an old drag's release).
    pub(crate) fn drag_listeners(&self, cx: &Context<Self>) -> impl IntoElement {
        let pressing = self.drag.pressing.clone();
        let last_scroll = self.drag.last_scroll.clone();
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        last_scroll.set(Some(Instant::now()));
                    }
                });
                let (moving, mover) = (pressing.clone(), this.clone());
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && moving.get() {
                        let _ = mover.update(cx, |this, cx| this.drag_move(event, cx));
                    }
                });
                let presser = this.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    // Capture runs before the handlers that set it for a menu they close.
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Right {
                        let _ = presser.update(cx, |this, _| this.swallow_right_up = false);
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && pressing.get() {
                        let _ = this.update(cx, |this, _| this.drag.end_press());
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// A button went down on list row `row`. Like Explorer, a row outside the selection is
    /// selected at once (without modifiers), so that it is what a drag takes.
    pub(crate) fn press_row(&mut self, row: usize, event: &MouseDownEvent, button: Button) {
        self.drag.dragged = false;
        let tab = self.tab();
        let in_selection = tab.selection.contains(row as u32);
        if !in_selection && button == Button::Left && !event.modifiers.modified() {
            self.select_row(row, false, false);
        }
        let tab = self.tab();
        let Some(snapshot) = tab.snapshot() else {
            return;
        };
        // Folders, files: what the drag would carry.
        let kind = |row: usize| snapshot.row(row).map(|entry| entry.kind.is_container());
        let (mut folders, mut files) = (false, false);
        let rows: Box<dyn Iterator<Item = usize>> = if tab.selection.contains(row as u32) {
            Box::new(tab.selection.iter().map(|selected| selected as usize))
        } else {
            Box::new(std::iter::once(row))
        };
        for row in rows {
            match kind(row) {
                Some(true) => folders = true,
                Some(false) => files = true,
                None => {}
            }
            if folders && files {
                break;
            }
        }
        self.press(
            Pressed::Row(row),
            folders,
            files,
            DragOrigin::Content,
            button,
            event,
        );
    }

    /// A button went down on a sidebar item; only folders (pins and drives) can be dragged.
    pub(crate) fn press_sidebar(&mut self, to: &Location, event: &MouseDownEvent, button: Button) {
        self.drag.dragged = false;
        let Some(path) = to.as_path().map(Path::to_path_buf) else {
            return;
        };
        self.press(
            Pressed::Sidebar(path),
            true,
            false,
            DragOrigin::Sidebar,
            button,
            event,
        );
    }

    fn press(
        &mut self,
        what: Pressed,
        folders: bool,
        files: bool,
        origin: DragOrigin,
        button: Button,
        event: &MouseDownEvent,
    ) {
        // Inside a rename field a press selects text; it never drags the item.
        if self.ops.renaming.is_some() {
            return;
        }
        let policy = self.drag.shared.policy.get();
        let allowed = policy.may_start(folders, files, origin, button);
        let now = self.drag.now_ms();
        let last_scroll = self.drag.last_scroll.get().map(|at| self.drag.ms_of(at));
        let modifiers = Modifiers {
            ctrl: event.modifiers.control,
            shift: event.modifiers.shift,
            alt: event.modifiers.alt,
        };
        let at = (f32::from(event.position.x), f32::from(event.position.y));
        self.drag
            .guard
            .press(&policy, now, at.0, at.1, modifiers, last_scroll, allowed);
        let missing_key = policy.require_modifier.filter(|key| !match key {
            Modifier::Alt => modifiers.alt,
            Modifier::Ctrl => modifiers.ctrl,
            Modifier::Shift => modifiers.shift,
        });
        self.drag.press = Some(Press {
            what,
            button,
            policy,
            at,
            missing_key,
            mixed: folders && files,
        });
        self.drag.pressing.set(true);
    }

    fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(press) = self.drag.press.clone() else {
            return;
        };
        // Released somewhere the release was not seen (another window).
        if event.pressed_button.is_none() {
            self.drag.end_press();
            return;
        }
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        let now = self.drag.now_ms();
        match self.drag.guard.moved(now, x, y) {
            Step::StartDrag => self.start_drag(cx),
            Step::Hint(blocked) => self.drag_hint(blocked, press.mixed, cx),
            Step::Wait => {
                // Held back by the key `dnd.require_modifier` asks for: say which.
                let far = (x - press.at.0).hypot(y - press.at.1) >= press.policy.threshold();
                if let Some(key) = press.missing_key
                    && far
                    && self.features.dnd_blocked_hint
                    && self.drag.may_hint("dnd.require_modifier")
                {
                    self.show_key_hint(key, cx);
                }
            }
            Step::Click | Step::Dropped => {}
        }
    }

    /// The press became a drag: the items go out through OLE, on the modal host.
    fn start_drag(&mut self, cx: &mut Context<Self>) {
        let Some(press) = self.drag.press.take() else {
            return;
        };
        self.drag.pressing.set(false);
        let paths = match &press.what {
            Pressed::Row(row) => {
                if self.tab().selection.contains(*row as u32) {
                    self.selected_paths()
                } else {
                    self.row_path(*row).into_iter().collect()
                }
            }
            Pressed::Sidebar(path) => vec![path.clone()],
        };
        let Some(owner) = self.hwnd else {
            self.drag.end_press();
            return;
        };
        if paths.is_empty() {
            self.drag.end_press();
            return;
        }
        let right = press.button == Button::Right;
        // A drive is offered as a shortcut only (as in Explorer): never a whole-drive copy or
        // move. Read-only mode: nothing leaves this folder by a move. Sidebar items follow
        // `dnd.source.sidebar` (and the anti-drag preset turns that off).
        let drive = paths.iter().any(|path| path.parent().is_none());
        let from_sidebar = matches!(press.what, Pressed::Sidebar(_));
        let allowed = Allowed {
            copy: !drive,
            moves: !drive && !press.policy.readonly,
            link: true,
        };
        let leave_window = press.policy.external_out;
        let outgoing = Outgoing {
            from_sidebar,
            policy: press.policy,
        };
        // The release that ends a right-button drag opens no menu.
        if right {
            self.swallow_right_up = true;
        }
        self.drag.dragged = true;
        cx.spawn(async move |this, cx| {
            // Many items, or items in a network folder, take time to parse: off the UI thread.
            // The drag itself checks that the button is still down when it starts.
            let prepared = crate::ops::off_thread(move || sx_shell::dnd::prepare(&paths)).await;
            let items = match prepared {
                Some(Ok(items)) => items,
                failed => {
                    if let Some(Err(err)) = failed {
                        tracing::warn!(error = %err, "drag items");
                    }
                    let _ = this.update(cx, |this, cx| {
                        this.drag.end_press();
                        this.notice = Some(rust_i18n::t!("dnd.drag_failed").into());
                        cx.notify();
                    });
                    return;
                }
            };
            let Ok(done) = this.update(cx, |this, _| {
                this.drag.shared.outgoing.set(Some(outgoing));
                let request = DragOut {
                    owner,
                    items,
                    allowed,
                    leave_window,
                    right,
                };
                sx_shell::modal::run(move || sx_shell::dnd::drag(&request))
            }) else {
                return;
            };
            let result = done.await;
            let _ = this.update(cx, |this, cx| {
                this.drag.shared.outgoing.set(None);
                this.drag.end_press();
                match result {
                    Ok(Ok(effect)) => tracing::debug!(?effect, "drag ended"),
                    Ok(Err(err)) => {
                        tracing::warn!(error = %err, "drag failed");
                        this.notice = Some(rust_i18n::t!("dnd.drag_failed").into());
                    }
                    Err(_) => tracing::warn!("drag did not run (no modal host)"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The path of a list row.
    fn row_path(&self, row: usize) -> Option<PathBuf> {
        let tab = self.tab();
        let snapshot = tab.snapshot()?;
        let entry = snapshot.row(row)?;
        let dir = tab.location().as_path().unwrap_or_else(|| Path::new(""));
        snapshot.path_of(dir, entry.index)
    }

    /// The user tried to drag something a switch blocks (`dnd.blocked_hint`, DESIGN.md §9.3).
    fn drag_hint(&mut self, blocked: Blocked, mixed: bool, cx: &mut Context<Self>) {
        let flag = match blocked {
            Blocked::AllOff => "dnd.enabled",
            Blocked::Files => "dnd.source.files",
            Blocked::Folders => "dnd.source.folders",
            Blocked::Sidebar => "dnd.source.sidebar",
        };
        if self.features.dnd_blocked_hint && self.drag.may_hint(flag) {
            self.show_blocked_hint(flag, mixed, cx);
        }
    }

    fn on_drop_event(&mut self, event: DropEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            DropEvent::Hover(hover) => {
                if self.drag.hover != hover {
                    self.drag.hover = hover;
                    cx.notify();
                }
            }
            DropEvent::Refused(flag) => {
                if self.features.dnd_blocked_hint && self.drag.may_hint(flag) {
                    self.show_blocked_hint(flag, false, cx);
                }
            }
            DropEvent::Drop {
                paths,
                dest,
                action,
                allowed,
                at,
            } => {
                self.drag.hover = None;
                let quiet = !self.features.dnd_undo_toast;
                match action {
                    DropAction::Copy => self.transfer(paths, dest, Mode::Copy, quiet, cx),
                    DropAction::Move => self.move_dropped(paths, dest, quiet, cx),
                    DropAction::Ask => self.open_drop_menu(paths, dest, at, allowed, window, cx),
                }
                cx.notify();
            }
        }
    }

    /// Moves dropped items, asking first when they include folders and
    /// `dnd.confirm_folder_move` is on (the drop target cannot look at the items in time).
    pub(crate) fn move_dropped(
        &mut self,
        paths: Vec<PathBuf>,
        dest: PathBuf,
        quiet: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.features.dnd_confirm_folder_move {
            self.transfer(paths, dest, Mode::Move, quiet, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let list = paths.clone();
            let folders = crate::ops::off_thread(move || list.iter().any(|p| p.is_dir()))
                .await
                .unwrap_or(true);
            let _ = this.update(cx, |this, cx| {
                if !folders {
                    this.transfer(paths, dest, Mode::Move, quiet, cx);
                } else if this.ops.dialog.is_none() {
                    this.ops.dialog = Some(crate::ops::Dialog::ConfirmMove {
                        sources: paths,
                        dest,
                        quiet,
                    });
                    cx.notify();
                } else {
                    // Never in place of the dialog already open (its question would be lost).
                    this.notice = Some(rust_i18n::t!("ops.busy_dialog").into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The drop highlight of a list row (entry `index` of tab `tab`): `Some(blocked)`.
    pub(crate) fn row_drop(&self, tab: u64, index: u32) -> Option<bool> {
        match &self.drag.hover {
            Some((Place::Row { tab: t, index: i }, blocked)) if *t == tab && *i == index => {
                Some(*blocked)
            }
            _ => None,
        }
    }

    /// The drop highlight of the open folder of tab `tab`: `Some(blocked)`.
    pub(crate) fn folder_drop(&self, tab: u64) -> Option<bool> {
        match &self.drag.hover {
            Some((Place::Folder { tab: t }, blocked)) if *t == tab => Some(*blocked),
            _ => None,
        }
    }

    /// The drop highlight of a sidebar item: `Some(blocked)`.
    pub(crate) fn sidebar_drop(&self, to: &Location) -> Option<bool> {
        match &self.drag.hover {
            Some((Place::Sidebar(place), blocked)) if place == to => Some(*blocked),
            _ => None,
        }
    }
}

/// The OLE side's view of the window (see [`Shared`]).
struct Router(Rc<Shared>);

impl Router {
    fn logical(&self, at: ClientPoint) -> Point<Pixels> {
        let scale = self.0.scale.get().max(0.5);
        point(px(at.x as f32 / scale), px(at.y as f32 / scale))
    }

    /// The folder under the pointer: a sidebar item, a folder row, or the open folder.
    fn spot(&self, at: ClientPoint) -> Option<Spot> {
        let at = self.logical(at);
        // The sidebar first: it is drawn beside the list, never under it. Its items record
        // only their visible part, so one scrolled out of view takes nothing.
        if let Ok(targets) = self.0.sidebar.try_borrow() {
            for target in targets.iter() {
                if target
                    .bounds
                    .get()
                    .is_some_and(|bounds| bounds.contains(&at))
                {
                    // Home, This PC and the Recycle Bin take no drops (yet).
                    let dest = target.location.as_path()?.to_path_buf();
                    return Some(Spot {
                        place: Place::Sidebar(target.location.clone()),
                        dest,
                        target: DropTarget::Sidebar,
                    });
                }
            }
        }
        let list = self.0.list.try_borrow().ok()?;
        let list = list.as_ref()?;
        if !self.0.pane.get()?.contains(&at) {
            return None;
        }
        if let Some(rows) = list.rows.get()
            && rows.contains(&at)
        {
            let row = (at.y - rows.top() - list.offset.get()) / list.row_height;
            if row >= 0.0
                && let Some(entry) = list.snapshot.row(row.floor() as usize)
                && entry.kind.is_container()
            {
                let dir = list.folder.as_deref().unwrap_or_else(|| Path::new(""));
                let dest = list.snapshot.path_of(dir, entry.index)?;
                return Some(Spot {
                    place: Place::Row {
                        tab: list.tab,
                        index: entry.index,
                    },
                    dest,
                    target: DropTarget::Folder,
                });
            }
        }
        Some(Spot {
            place: Place::Folder { tab: list.tab },
            dest: list.folder.clone()?,
            target: DropTarget::Folder,
        })
    }

    /// Where the drag would land, what the source is shown, and what a drop would do.
    fn decide(&self, at: ClientPoint, keys: Keys, allowed: Allowed) -> Decision {
        if self.0.busy.get() {
            return Decision::nothing();
        }
        let Ok(mut incoming) = self.0.incoming.try_borrow_mut() else {
            return Decision::nothing();
        };
        let Some(spot) = self.spot(at) else {
            return Decision::nothing();
        };
        let outgoing = self.0.outgoing.get();
        // Our own drag keeps the switches of its press (DEVELOPER-MODE.md §5.4).
        let policy = outgoing.map_or_else(|| self.0.policy.get(), |own| own.policy);
        // From this window, or from another of ours: inside the app either way.
        let internal = outgoing.is_some() || sx_shell::dnd::dragging_out();
        if let Some(flag) = refusal(&policy, internal, spot.target) {
            return Decision::refused(spot.place, flag);
        }
        // A pin dropped on a sidebar item would move the pinned folder: not until pins can be
        // reordered by dragging (which never moves files).
        if outgoing.is_some_and(|own| own.from_sidebar) && spot.target == DropTarget::Sidebar {
            return Decision::nothing();
        }
        // A whole drive or share is never copied or moved by a drop (shortcuts come later).
        if incoming.paths.is_empty() || incoming.roots {
            return Decision::nothing();
        }
        let (inside, here) = incoming.relation(&spot.dest);
        // Into itself or a folder inside it: never.
        if inside {
            return Decision::nothing();
        }
        let modifiers = Modifiers {
            ctrl: keys.ctrl,
            shift: keys.shift,
            alt: keys.alt,
        };
        let button = if keys.right {
            Button::Right
        } else {
            Button::Left
        };
        let same_volume = incoming.volume.is_some() && incoming.volume == volume(&spot.dest);
        // Whether folders are among the items is looked at when a move runs (the confirmation
        // of `dnd.confirm_folder_move`), not here: that would read the disk on every move.
        let mut effect = policy.effect(spot.target, modifiers, button, same_volume, false);
        let asked = keys.ctrl || keys.shift || keys.alt;
        // A source that refuses a move (read-only media, a browser) gets a copy, unless a key
        // asked for the move.
        if matches!(effect, DropEffect::Move | DropEffect::ConfirmMove)
            && !allowed.moves
            && allowed.copy
            && !asked
        {
            effect = DropEffect::Copy;
        }
        let shown = match effect {
            DropEffect::Copy => Effect::Copy,
            // Moving items to where they already are does nothing.
            DropEffect::Move | DropEffect::ConfirmMove if !here => Effect::Move,
            // A left drag let go over the folder it came from: nothing, no menu (as Explorer).
            // With the right button the menu still offers a copy.
            DropEffect::Ask if here && !keys.right => Effect::None,
            DropEffect::Ask if allowed.copy => Effect::Copy,
            DropEffect::Ask if allowed.moves && !here => Effect::Move,
            // Shortcuts are not made yet.
            _ => Effect::None,
        };
        if shown == Effect::None || !allowed.allows(shown) {
            return Decision::nothing();
        }
        Decision {
            hover: Some((spot.place.clone(), false)),
            shown,
            drop: Some((spot, effect)),
            refused_by: None,
            confirm_moves: policy.confirm_folder_move,
        }
    }

    fn show(&self, decision: &Decision) {
        if let Ok(mut current) = self.0.hover.try_borrow_mut()
            && *current != decision.hover
        {
            current.clone_from(&decision.hover);
            let _ = self
                .0
                .events
                .unbounded_send(DropEvent::Hover(decision.hover.clone()));
        }
        if decision.refused_by != self.0.refused.get() {
            self.0.refused.set(decision.refused_by);
            if let Some(flag) = decision.refused_by {
                let _ = self.0.events.unbounded_send(DropEvent::Refused(flag));
            }
        }
    }
}

/// The switch that refuses a drop on `target` (`boleh_drop`, `boleh_masuk`), if one does.
fn refusal(policy: &DragPolicy, internal: bool, target: DropTarget) -> Option<&'static str> {
    if policy.readonly {
        return Some("safety.readonly_mode");
    }
    if !internal {
        if !policy.enabled {
            return Some("dnd.enabled");
        }
        if !policy.external_in {
            return Some("dnd.external_in");
        }
    }
    match target {
        DropTarget::Folder if !policy.target_folders => Some("dnd.target.folders"),
        DropTarget::Sidebar if !policy.target_sidebar => Some("dnd.target.sidebar"),
        _ => None,
    }
}

impl DropHandler for Router {
    fn enter(&self, paths: Vec<PathBuf>, at: ClientPoint, keys: Keys, allowed: Allowed) -> Effect {
        if let Ok(mut incoming) = self.0.incoming.try_borrow_mut() {
            *incoming = Incoming::new(paths);
        }
        self.0.refused.set(None);
        self.over(at, keys, allowed)
    }

    fn over(&self, at: ClientPoint, keys: Keys, allowed: Allowed) -> Effect {
        let decision = self.decide(at, keys, allowed);
        self.show(&decision);
        decision.shown
    }

    fn leave(&self) {
        if let Ok(mut incoming) = self.0.incoming.try_borrow_mut() {
            *incoming = Incoming::default();
        }
        self.show(&Decision::nothing());
    }

    fn dropped(
        &self,
        paths: Vec<PathBuf>,
        at: ClientPoint,
        keys: Keys,
        allowed: Allowed,
    ) -> DropReply {
        if let Ok(mut incoming) = self.0.incoming.try_borrow_mut() {
            *incoming = Incoming::new(paths);
        }
        let Decision {
            shown,
            drop,
            confirm_moves,
            ..
        } = self.decide(at, keys, allowed);
        let paths = self
            .0
            .incoming
            .try_borrow_mut()
            .map(|mut incoming| std::mem::take(&mut incoming.paths))
            .unwrap_or_default();
        self.leave();
        let Some((spot, effect)) = drop else {
            return DropReply::Refused;
        };
        let (action, reply) = match (effect, shown) {
            (DropEffect::Copy, Effect::Copy) => (DropAction::Copy, DropReply::Copy),
            // A confirmation may still stop it: the source hears only that we take over.
            (DropEffect::Move | DropEffect::ConfirmMove, Effect::Move) if confirm_moves => {
                (DropAction::Move, DropReply::Later)
            }
            (DropEffect::Move | DropEffect::ConfirmMove, Effect::Move) => {
                (DropAction::Move, DropReply::Move)
            }
            (DropEffect::Ask, _) => (DropAction::Ask, DropReply::Later),
            _ => return DropReply::Refused,
        };
        let sent = self.0.events.unbounded_send(DropEvent::Drop {
            paths,
            dest: spot.dest,
            action,
            allowed,
            at: self.logical(at),
        });
        // Nobody to run it: the source must not think it was done.
        if sent.is_err() {
            return DropReply::Refused;
        }
        reply
    }
}

/// The drop outline of an item or a pane (DESIGN.md §11), drawn over it so it takes no room:
/// `drop` is `Some(blocked)` while a drag is over it; blocked shows `drop_blocked.border` only.
pub(crate) fn drop_ring(drop: Option<bool>, radius: Token, cx: &gpui_kit::App) -> Option<Div> {
    let blocked = drop?;
    let t = cx.tokens();
    Some(
        div()
            .absolute()
            .inset_0()
            .rounded(t.px(radius))
            .border_1()
            .border_color(t.color(if blocked {
                Token::DropBlockedBorder
            } else {
                Token::DropTargetBorder
            })),
    )
}

/// Whether the path with key `dest` is the path with key `source` or inside it.
fn within(dest: &str, source: &str) -> bool {
    dest == source
        || (dest.len() > source.len()
            && dest.starts_with(source)
            && dest.as_bytes().get(source.len()) == Some(&b'\\'))
}

/// A path for comparing (Windows paths: case does not matter): lower case, without a verbatim
/// prefix or a trailing separator.
fn key(path: &Path) -> String {
    let text = path.to_string_lossy().to_lowercase();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\unc\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    };
    text.trim_end_matches('\\').to_string()
}

/// The volume a path is on: its drive letter or its UNC share.
fn volume(path: &Path) -> Option<String> {
    match path.components().next()? {
        Component::Prefix(prefix) => Some(match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                format!("{}:", char::from(letter).to_ascii_lowercase())
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                r"\\{}\{}",
                server.to_string_lossy().to_lowercase(),
                share.to_string_lossy().to_lowercase()
            ),
            _ => prefix.as_os_str().to_string_lossy().to_lowercase(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(dest: &str, source: &str) -> bool {
        within(&key(Path::new(dest)), &key(Path::new(source)))
    }

    #[test]
    fn a_folder_is_inside_itself_and_its_parents_only() {
        let source = r"C:\Data\Reports";
        assert!(inside(r"C:\Data\Reports", source));
        assert!(inside(r"c:\data\reports\2026", source));
        assert!(inside(r"\\?\C:\Data\Reports\old", source));
        assert!(!inside(r"C:\Data\Reports 2026", source));
        assert!(!inside(r"C:\Data", source));
        // A drive holds everything on it.
        assert!(inside(r"C:\Data", r"C:\"));
    }

    #[test]
    fn volumes_compare_by_drive_or_share() {
        assert_eq!(volume(Path::new(r"C:\a")), volume(Path::new(r"\\?\c:\b")));
        assert_ne!(volume(Path::new(r"C:\a")), volume(Path::new(r"D:\a")));
        assert_eq!(
            volume(Path::new(r"\\Server\Share\a")),
            volume(Path::new(r"\\server\share\b"))
        );
        assert_ne!(
            volume(Path::new(r"\\server\one\a")),
            volume(Path::new(r"\\server\two\a"))
        );
    }

    #[test]
    fn items_know_whether_they_are_already_in_a_folder() {
        let mut incoming = Incoming::new(vec![
            PathBuf::from(r"C:\Data\a.txt"),
            PathBuf::from(r"c:\data\B.txt"),
        ]);
        assert_eq!(incoming.relation(Path::new(r"C:\Data")), (false, true));
        assert_eq!(incoming.relation(Path::new(r"C:\Data\Sub")), (false, false));
        let mut folders = Incoming::new(vec![PathBuf::from(r"C:\Data\Sub")]);
        assert_eq!(
            folders.relation(Path::new(r"C:\Data\Sub\Deeper")),
            (true, false)
        );
        let mut spread = Incoming::new(vec![
            PathBuf::from(r"C:\One\a.txt"),
            PathBuf::from(r"C:\Two\b.txt"),
        ]);
        assert_eq!(spread.relation(Path::new(r"C:\One")), (false, false));
    }

    #[test]
    fn switches_refuse_drops_by_name() {
        let policy = DragPolicy::default();
        assert_eq!(refusal(&policy, false, DropTarget::Folder), None);
        let readonly = DragPolicy {
            readonly: true,
            ..policy
        };
        assert_eq!(
            refusal(&readonly, true, DropTarget::Folder),
            Some("safety.readonly_mode")
        );
        let closed = DragPolicy {
            external_in: false,
            ..policy
        };
        assert_eq!(
            refusal(&closed, false, DropTarget::Folder),
            Some("dnd.external_in")
        );
        assert_eq!(refusal(&closed, true, DropTarget::Folder), None);
        let no_sidebar = DragPolicy {
            target_sidebar: false,
            ..policy
        };
        assert_eq!(
            refusal(&no_sidebar, true, DropTarget::Sidebar),
            Some("dnd.target.sidebar")
        );
    }

    #[test]
    fn the_policy_follows_the_switches() {
        let features = Features {
            dnd_source_folders: false,
            dnd_default_action: "ask",
            dnd_require_modifier: "alt",
            safety_readonly_mode: true,
            ..Features::default()
        };
        let policy = policy(&features, 4.0);
        assert!(!policy.source_folders);
        assert_eq!(policy.default_action, DefaultAction::Ask);
        assert_eq!(policy.require_modifier, Some(Modifier::Alt));
        assert!(policy.readonly);
        assert!((policy.threshold() - 4.0).abs() < f32::EPSILON);
    }
}
