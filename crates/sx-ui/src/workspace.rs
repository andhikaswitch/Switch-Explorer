//! The main window content (DESIGN.md §8.1): title bar with tabs, command bar, sidebar, the
//! details view and the status bar. State and behaviour live here; each region renders from
//! its own module.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, Styled as _, Subscription, Window,
    WindowBackgroundAppearance, div,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rust_i18n::t;
use sx_core::{EntryKind, Location};
use sx_fs::{Address, Column, LoadError, SortSpec, ViewOptions};
use sx_settings::Features;
use sx_shell::OsAppearance;
use sx_theme::{Mode, Token};
use sx_win::{Drive, KnownFolder};

use crate::actions::{self, WORKSPACE_CONTEXT};
use crate::tab::{Content, Tab};
use crate::theme::{ActiveTokens as _, ThemeController};

/// Folder changes are collected for this long before the listing reloads, so a burst of
/// changes (an extraction, a copy) costs one reload.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Delay before "Loading…" appears, so fast folders do not flicker (DESIGN.md §11).
pub(crate) const LOADING_DELAY: Duration = Duration::from_millis(150);

/// A request from another process (single instance, `OpenPaths`) or the command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenRequest {
    /// Folders to open, one tab each.
    pub paths: Vec<PathBuf>,
    /// An item to show: its folder opens and the item is selected.
    pub select: Option<PathBuf>,
    /// Open Home too.
    pub home: bool,
}

/// What the window starts with.
#[derive(Debug)]
pub struct WorkspaceOptions {
    /// One tab per location; Home when empty.
    pub locations: Vec<Location>,
    pub features: Arc<Features>,
    /// The OS supports system backdrops (Windows 11 22H2+).
    pub backdrop_supported: bool,
    pub appearance: OsAppearance,
    /// `--appearance light|dark`: ignore the Windows app theme for this session.
    pub forced_light: Option<bool>,
    /// `--bench-open`: process start; the first listing prints its timings and quits.
    pub bench_started: Option<Instant>,
    /// Requests forwarded by later launches (single instance).
    pub requests: Option<mpsc::UnboundedReceiver<OpenRequest>>,
}

/// A pinned sidebar item.
#[derive(Clone, Debug)]
pub struct Pin {
    pub name: SharedString,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct Workspace {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    next_tab_id: u64,
    pub(crate) list_focus: FocusHandle,
    pub(crate) address: gpui_kit::Entity<InputState>,
    pub(crate) editing_address: bool,
    pub(crate) drives: Vec<Drive>,
    pub(crate) pins: Vec<Pin>,
    pub(crate) sidebar_visible: bool,
    /// The sidebar item or card under the pointer. Their children (icon, caption, gauge) take
    /// the highlight colours from this; GPUI group hover is not known yet when text is laid out.
    pub(crate) hovered: Option<gpui_kit::ElementId>,
    pub(crate) features: Arc<Features>,
    /// One-line message about the last failed action (cause + fix, DESIGN.md §10).
    pub(crate) notice: Option<SharedString>,
    hwnd: Option<isize>,
    backdrop_supported: bool,
    appearance: OsAppearance,
    forced_light: Option<bool>,
    bench_started: Option<Instant>,
    bench_report: Option<BenchReport>,
    /// Watchers report changed tabs here; `pending_refresh` debounces them.
    changes: mpsc::UnboundedSender<u64>,
    pending_refresh: HashSet<u64>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug)]
struct BenchReport {
    process: Instant,
    listing: Instant,
    listing_ms: f64,
    entries: usize,
}

impl BenchReport {
    /// `listing_ms` is enumeration + sort; `first_rows_ms` adds the frame that shows the rows;
    /// `process_ms` counts from `main`.
    fn print(self) {
        let line = format!(
            "bench-open entries={} listing_ms={:.1} first_rows_ms={:.1} process_ms={:.1}",
            self.entries,
            self.listing_ms,
            self.listing.elapsed().as_secs_f64() * 1000.0,
            self.process.elapsed().as_secs_f64() * 1000.0,
        );
        tracing::info!("{line}");
        println!("{line}");
    }
}

impl Workspace {
    pub fn new(options: WorkspaceOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("nav.address_placeholder").to_string())
        });
        let subscriptions = vec![cx.subscribe_in(&address, window, Self::on_address_event)];
        let list_focus = cx.focus_handle();
        window.focus(&list_focus, cx);
        let (changes, changed_tabs) = mpsc::unbounded();

        let mut this = Self {
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 1,
            list_focus,
            address,
            editing_address: false,
            drives: Vec::new(),
            pins: default_pins(),
            sidebar_visible: true,
            hovered: None,
            features: options.features,
            notice: None,
            hwnd: hwnd(window),
            backdrop_supported: options.backdrop_supported,
            appearance: options.appearance,
            forced_light: options.forced_light,
            bench_started: options.bench_started,
            bench_report: None,
            changes,
            pending_refresh: HashSet::new(),
            _subscriptions: subscriptions,
        };
        let locations = if options.locations.is_empty() {
            vec![Location::Home]
        } else {
            options.locations
        };
        for location in locations {
            this.open_tab(location, cx);
        }
        this.active = 0;
        this.refresh_drives(cx);
        this.watch_appearance(window, cx);
        this.watch_changes(changed_tabs, cx);
        if let Some(requests) = options.requests {
            this.watch_requests(requests, window, cx);
        }
        this.sync_window(window);
        this
    }

    /// Records the hovered sidebar item or card (see `hovered`).
    pub(crate) fn set_hovered(
        &mut self,
        id: &gpui_kit::ElementId,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if hovered {
            if self.hovered.as_ref() != Some(id) {
                self.hovered = Some(id.clone());
                cx.notify();
            }
        } else if self.hovered.as_ref() == Some(id) {
            self.hovered = None;
            cx.notify();
        }
    }

    pub(crate) fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub(crate) fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    // ---- Appearance ---------------------------------------------------------------------

    fn watch_requests(
        &mut self,
        mut requests: mpsc::UnboundedReceiver<OpenRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.next().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    this.open_request(request, cx);
                    window.activate_window();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Opens what another launch asked for: one tab per folder; `select` opens the item's folder
    /// and selects it. A request with nothing in it just brings the window forward.
    pub(crate) fn open_request(&mut self, request: OpenRequest, cx: &mut Context<Self>) {
        if request.home {
            self.open_tab(Location::Home, cx);
        }
        for path in request.paths {
            self.open_tab(Location::Path(path), cx);
        }
        if let Some(item) = request.select
            && let Some(parent) = item.parent()
        {
            self.open_tab(Location::Path(parent.to_path_buf()), cx);
            self.tab_mut().reveal = item.file_name().map(|n| n.to_string_lossy().into_owned());
        }
        cx.notify();
    }

    fn watch_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (tx, mut rx) = mpsc::unbounded();
        sx_shell::appearance::watch(move |appearance| tx.unbounded_send(appearance).is_ok());
        cx.spawn_in(window, async move |this, cx| {
            while let Some(appearance) = rx.next().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    this.on_appearance(appearance, window, cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_appearance(&mut self, mut appearance: OsAppearance, window: &mut Window, cx: &mut App) {
        if let Some(light) = self.forced_light {
            appearance.light = light;
        }
        self.appearance = appearance;
        let (mode, solid) = self.material();
        ThemeController::apply(mode, solid, cx);
        self.sync_window(window);
    }

    /// Light/dark and solid material from the OS (THEMING.md §10, DESIGN.md §6).
    pub fn material(&self) -> (Mode, bool) {
        let mode = if self.appearance.light {
            Mode::Light
        } else {
            Mode::Dark
        };
        (
            mode,
            !(self.backdrop_supported && self.appearance.transparency),
        )
    }

    /// Backdrop and frame colour follow the theme; GPUI does not do this when the change came
    /// from the registry watch (SYSTEM-INTEGRATION.md §3).
    fn sync_window(&self, window: &mut Window) {
        let (mode, solid) = self.material();
        window.set_background_appearance(if solid {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::MicaAltBackdrop
        });
        if let Some(hwnd) = self.hwnd {
            sx_shell::dwm::set_dark_frame(hwnd, mode == Mode::Dark);
        }
    }

    // ---- Tabs ---------------------------------------------------------------------------

    pub(crate) fn open_tab(&mut self, location: Location, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::new(id, location));
        self.active = self.tabs.len() - 1;
        self.load(cx);
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active = ix;
            self.leave_address(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            window.remove_window();
            return;
        }
        self.tabs[ix].cancel_load();
        self.tabs.remove(ix);
        if self.active >= self.tabs.len() || self.active > ix {
            self.active = self.active.saturating_sub(1);
        }
        cx.notify();
    }

    // ---- Navigation ---------------------------------------------------------------------

    pub(crate) fn navigate(&mut self, to: Location, cx: &mut Context<Self>) {
        self.notice = None;
        self.hovered = None;
        if self.tab_mut().history.navigate(to) {
            self.tab_mut().reveal = None;
            self.load(cx);
        }
    }

    pub(crate) fn go_up(&mut self, cx: &mut Context<Self>) {
        let current = self.tab().location().clone();
        let Some(parent) = current.parent() else {
            return;
        };
        let child = current
            .as_path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        self.navigate(parent, cx);
        self.tab_mut().reveal = child;
    }

    /// (Re)loads the active tab's location on a background executor.
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        let id = self.tab().id;
        self.load_tab(id, false, cx);
    }

    /// Loads a tab's location. A `refresh` keeps the current listing on screen until the new
    /// one is ready and then keeps the selection by name (watcher changes, F5 after an
    /// operation).
    pub(crate) fn load_tab(&mut self, tab_id: u64, refresh: bool, cx: &mut Context<Self>) {
        let features = self.features.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let refresh = refresh && matches!(tab.content, Content::Folder(_));
        if !refresh {
            tab.watcher = None;
        }
        let cancel = tab.begin_load();
        let Location::Path(path) = tab.location().clone() else {
            tab.content = Content::Page;
            tab.selection.clear();
            cx.notify();
            return;
        };
        if !refresh {
            tab.content = Content::Loading {
                since: Instant::now(),
            };
        }
        let sort = SortSpec {
            folders_first: features.view_folders_first,
            natural: features.view_natural_sort,
            ..tab.sort
        };
        tab.sort = sort;
        let view = ViewOptions {
            show_hidden: features.view_show_hidden,
            show_system: features.view_show_system,
            filter: String::new(),
        };
        cx.notify();
        let started = Instant::now();
        let listing = cx.background_executor().spawn({
            let path = path.clone();
            async move { sx_fs::load(&path, sort, view, &cancel) }
        });
        let delay = cx.background_executor().timer(LOADING_DELAY);
        cx.spawn(async move |this, cx| {
            delay.await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = listing.await;
            let _ = this.update(cx, |this, cx| {
                this.on_loaded(tab_id, &path, result, started, refresh, cx);
            });
        })
        .detach();
    }

    fn on_loaded(
        &mut self,
        tab_id: u64,
        path: &std::path::Path,
        result: Result<sx_fs::DirSnapshot, LoadError>,
        started: Instant,
        refresh: bool,
        cx: &mut Context<Self>,
    ) {
        let watch = self.features.perf_watcher;
        let changes = self.changes.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        // A newer navigation in this tab wins.
        if tab.location().as_path() != Some(path) {
            return;
        }
        match result {
            Ok(snapshot) => {
                tracing::debug!(
                    entries = snapshot.len(),
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    "folder loaded"
                );
                let entries = snapshot.len();
                if refresh {
                    restore_selection(tab, snapshot);
                } else {
                    let reveal = tab.reveal.take();
                    let row = reveal.and_then(|name| {
                        (0..snapshot.visible_len())
                            .find(|&r| snapshot.row(r).is_some_and(|e| e.name == name))
                    });
                    tab.set_snapshot(Arc::new(snapshot));
                    tab.selection.clear();
                    if let Some(row) = row {
                        tab.selection.select_only(row as u32);
                        tab.scroll.scroll_to_item(row, ScrollStrategy::Center);
                    } else {
                        tab.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                }
                if watch && tab.watcher.is_none() {
                    tab.watcher =
                        sx_win::watch(path, move || changes.unbounded_send(tab_id).is_ok())
                            .map_err(
                                |err| tracing::debug!(error = %err, "folder watch unavailable"),
                            )
                            .ok();
                }
                if let Some(process) = self.bench_started.take() {
                    self.finish_bench(process, started, entries, cx);
                    return;
                }
            }
            Err(LoadError::Cancelled) => return,
            Err(err) if self.bench_started.is_some() => {
                tracing::error!(error = %err, "bench folder failed to load");
                cx.quit();
                return;
            }
            Err(err) => {
                tracing::debug!(error = %err, "folder failed to load");
                tab.content = Content::Failed(crate::format::load_error(&err));
            }
        }
        cx.notify();
    }

    /// `--bench-open`: the report is printed once the frame showing the listing is drawn
    /// (see `render`), then the app quits.
    fn finish_bench(
        &mut self,
        process: Instant,
        listing: Instant,
        entries: usize,
        cx: &mut Context<Self>,
    ) {
        self.bench_report = Some(BenchReport {
            process,
            listing,
            listing_ms: listing.elapsed().as_secs_f64() * 1000.0,
            entries,
        });
        cx.notify();
    }

    /// Reloads tabs whose folder changed, at most once per `WATCH_DEBOUNCE`.
    fn watch_changes(&mut self, mut changed: mpsc::UnboundedReceiver<u64>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(tab_id) = changed.next().await {
                let start = this.update(cx, |this, _| this.pending_refresh.insert(tab_id));
                match start {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(_) => break,
                }
                let timer = cx.background_executor().timer(WATCH_DEBOUNCE);
                let this = this.clone();
                cx.spawn(async move |cx| {
                    timer.await;
                    let _ = this.update(cx, |this, cx| {
                        this.pending_refresh.remove(&tab_id);
                        this.load_tab(tab_id, true, cx);
                    });
                })
                .detach();
            }
        })
        .detach();
    }

    pub(crate) fn refresh_drives(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async { sx_win::drives() });
        cx.spawn(async move |this, cx| {
            let drives = task.await;
            let _ = this.update(cx, |this, cx| {
                this.drives = drives;
                this.hovered = None;
                cx.notify();
            });
        })
        .detach();
    }

    // ---- Address bar --------------------------------------------------------------------

    pub(crate) fn edit_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = match self.tab().location() {
            Location::Path(path) => path.display().to_string(),
            other => crate::tab::location_title(other).to_string(),
        };
        self.editing_address = true;
        self.address.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn leave_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_address {
            self.editing_address = false;
            window.focus(&self.list_focus, cx);
            cx.notify();
        }
    }

    fn on_address_event(
        &mut self,
        input: &gpui_kit::Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                let text = input.read(cx).value().to_string();
                let current = self.tab().location().as_path().map(PathBuf::from);
                match sx_fs::address::parse(&text, current.as_deref()) {
                    Some(Address::Location(location)) => {
                        self.leave_address(window, cx);
                        self.navigate(location, cx);
                    }
                    Some(Address::Shell(_)) => {
                        self.notice = Some(t!("nav.shell_unsupported").into());
                        cx.notify();
                    }
                    None => {
                        self.notice = Some(t!("nav.invalid_address", input = text).into());
                        cx.notify();
                    }
                }
            }
            InputEvent::Blur => self.leave_address(window, cx),
            InputEvent::Change | InputEvent::Focus => {}
        }
    }

    // ---- Selection and opening ------------------------------------------------------------

    pub(crate) fn select_row(&mut self, row: usize, extend: bool, toggle: bool) {
        let tab = self.tab_mut();
        let row32 = row as u32;
        match (extend, toggle) {
            (true, additive) => tab.selection.extend_to(row32, additive),
            (false, true) => tab.selection.toggle(row32),
            (false, false) => tab.selection.select_only(row32),
        }
        tab.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
    }

    /// Moves the keyboard focus by `delta` rows (clamped), selecting or extending.
    pub(crate) fn move_selection(&mut self, delta: isize, extend: bool, focus_only: bool) {
        let len = self.tab().row_count();
        if len == 0 {
            return;
        }
        let focus = self.tab().selection.focus().map(|f| f as isize);
        let target = match focus {
            Some(f) => (f + delta).clamp(0, len as isize - 1),
            None if delta < 0 => len as isize - 1,
            None => 0,
        } as usize;
        if focus_only {
            let tab = self.tab_mut();
            tab.selection.move_focus(target as u32);
            tab.scroll.scroll_to_item(target, ScrollStrategy::Nearest);
        } else {
            self.select_row(target, extend, false);
        }
    }

    pub(crate) fn open_row(&mut self, row: usize, new_tab: bool, cx: &mut Context<Self>) {
        let tab = self.tab();
        let (Some(snapshot), Some(dir)) = (tab.snapshot(), tab.location().as_path()) else {
            return;
        };
        let Some(entry) = snapshot.row(row) else {
            return;
        };
        let Some(path) = snapshot.path_of(dir, entry.index) else {
            return;
        };
        let (kind, name) = (entry.kind, entry.name.to_string());
        if kind.is_container() {
            if new_tab {
                let active = self.active;
                self.open_tab(Location::Path(path), cx);
                self.active = active;
            } else {
                self.navigate(Location::Path(path), cx);
            }
            return;
        }
        let open = sx_shell::exec::open(path, self.hwnd);
        cx.spawn(async move |this, cx| {
            if let Err(err) = open.await
                && !err.is_cancelled()
            {
                tracing::debug!(error = %err, "open failed");
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(crate::format::open_error(&name, &err));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn open_selection(&mut self, new_tab: bool, cx: &mut Context<Self>) {
        let rows: Vec<usize> = self.tab().selection.iter().map(|r| r as usize).collect();
        let folders: Vec<usize> = rows
            .iter()
            .copied()
            .filter(|&r| {
                self.tab()
                    .snapshot()
                    .and_then(|s| s.row(r))
                    .is_some_and(|e| e.kind.is_container())
            })
            .collect();
        // One folder navigates; several folders open in tabs; files open with their app.
        if folders.len() == 1 && rows.len() == 1 {
            self.open_row(folders[0], new_tab, cx);
            return;
        }
        for row in rows {
            let is_folder = folders.contains(&row);
            self.open_row(row, is_folder || new_tab, cx);
        }
    }

    /// Re-sorts by a column (clicking the active column flips the direction). Sorting a large
    /// folder runs on a background executor (`DirSnapshot::sorted_order`); the selection is kept.
    pub(crate) fn sort_by(&mut self, column: Column, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        let spec = SortSpec {
            column,
            descending: tab.sort.column == column && !tab.sort.descending,
            ..tab.sort
        };
        tab.sort = spec;
        let Some(snapshot) = tab.snapshot().cloned() else {
            cx.notify();
            return;
        };
        let tab_id = tab.id;
        let task = cx
            .background_executor()
            .spawn(async move { snapshot.sorted_order(spec) });
        cx.spawn(async move |this, cx| {
            let order = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) else {
                    return;
                };
                let Content::Folder(snapshot) = &mut tab.content else {
                    return;
                };
                let selected: HashSet<u32> = tab
                    .selection
                    .iter()
                    .filter_map(|row| snapshot.row(row as usize).map(|e| e.index))
                    .collect();
                let focus = tab
                    .selection
                    .focus()
                    .and_then(|row| snapshot.row(row as usize).map(|e| e.index));
                Arc::make_mut(snapshot).apply_order(spec, order);
                let rows = (0..snapshot.visible_len() as u32).filter(|&r| {
                    snapshot
                        .row(r as usize)
                        .is_some_and(|e| selected.contains(&e.index))
                });
                let focus_row = focus.and_then(|ix| snapshot.row_of(ix)).map(|r| r as u32);
                let rows: Vec<u32> = rows.collect();
                tab.selection.restore(rows, focus_row);
                tab.clear_row_cache();
                if let Some(row) = focus_row {
                    tab.scroll
                        .scroll_to_item(row as usize, ScrollStrategy::Nearest);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn on_list_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return;
        }
        let Some(ch) = keystroke
            .key_char
            .as_deref()
            .and_then(|s| s.chars().next())
            .filter(|c| !c.is_control() && *c != ' ')
        else {
            return;
        };
        if let Some(row) = self.tab_mut().type_ahead(ch, Instant::now()) {
            self.select_row(row, false, false);
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// Whether the active tab shows a folder whose entry at `row` is a container.
    pub(crate) fn row_kind(&self, row: usize) -> Option<EntryKind> {
        self.tab().snapshot()?.row(row).map(|e| e.kind)
    }
}

/// Installs a reloaded listing, keeping the selected and focused entries by name.
fn restore_selection(tab: &mut Tab, snapshot: sx_fs::DirSnapshot) {
    let (selected, focus): (HashSet<String>, Option<String>) = match tab.snapshot() {
        Some(old) => (
            tab.selection
                .iter()
                .filter_map(|row| old.row(row as usize).map(|e| e.name.to_string()))
                .collect(),
            tab.selection
                .focus()
                .and_then(|row| old.row(row as usize).map(|e| e.name.to_string())),
        ),
        None => (HashSet::new(), None),
    };
    let rows: Vec<u32> = (0..snapshot.visible_len())
        .filter(|&r| snapshot.row(r).is_some_and(|e| selected.contains(e.name)))
        .map(|r| r as u32)
        .collect();
    let focus_row = focus.and_then(|name| {
        (0..snapshot.visible_len()).find(|&r| snapshot.row(r).is_some_and(|e| e.name == name))
    });
    tab.set_snapshot(Arc::new(snapshot));
    tab.selection.restore(rows, focus_row.map(|r| r as u32));
}

fn default_pins() -> Vec<Pin> {
    KnownFolder::PINNED_DEFAULTS
        .iter()
        .filter_map(|&folder| sx_win::known_folder(folder))
        .map(|path| Pin {
            name: path
                .file_name()
                .map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
                .into(),
            path,
        })
        .collect()
}

fn hwnd(window: &Window) -> Option<isize> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(report) = self.bench_report.take() {
            window.on_next_frame(move |_, cx| {
                report.print();
                cx.quit();
            });
        }
        let tokens = cx.tokens();
        let (font, fg, size, line) = (
            tokens.ui_font.clone(),
            tokens.color(Token::FgPrimary),
            tokens.px(Token::FontSizeBody),
            tokens.px(Token::FontLineBody),
        );
        div()
            .id("workspace")
            .key_context(WORKSPACE_CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .font_family(font)
            .text_color(fg)
            .text_size(size)
            .line_height(line)
            .on_action(cx.listener(|this, _: &actions::NavBack, _, cx| {
                if this.tab_mut().history.back().is_some() {
                    this.notice = None;
                    this.load(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &actions::NavForward, _, cx| {
                if this.tab_mut().history.forward().is_some() {
                    this.notice = None;
                    this.load(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &actions::NavUp, _, cx| this.go_up(cx)))
            .on_action(cx.listener(|this, _: &actions::NavRefresh, _, cx| {
                let id = this.tab().id;
                this.load_tab(id, true, cx);
                this.refresh_drives(cx);
            }))
            .on_action(cx.listener(|this, _: &actions::FocusAddress, window, cx| {
                this.edit_address(window, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::TabNew, window, cx| {
                this.open_tab(Location::Home, cx);
                this.leave_address(window, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::TabClose, window, cx| {
                this.close_tab(this.active, window, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::TabNext, window, cx| {
                let next = (this.active + 1) % this.tabs.len();
                this.activate_tab(next, window, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::TabPrevious, window, cx| {
                let previous = (this.active + this.tabs.len() - 1) % this.tabs.len();
                this.activate_tab(previous, window, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::ToggleSidebar, _, cx| {
                this.sidebar_visible = !this.sidebar_visible;
                this.hovered = None;
                cx.notify();
            }))
            .child(self.render_title_bar(window, cx))
            .child(self.render_command_bar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_visible, |el| el.child(self.render_sidebar(cx)))
                    .child(self.render_content(window, cx)),
            )
            .child(self.render_status_bar(cx))
    }
}

impl Workspace {
    /// The list's key handler, attached by the details view.
    pub(crate) fn list_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_list_key_down(event, window, cx);
    }
}
