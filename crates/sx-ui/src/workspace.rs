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
    App, AppContext as _, Context, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollStrategy, SharedString,
    Styled as _, Subscription, Window, WindowBackgroundAppearance, div,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rust_i18n::t;
use sx_core::{EntryKind, Location};
use sx_fs::{Address, Column, LoadError, SortSpec, ViewOptions};
use sx_settings::{Features, SettingsStore};
use sx_shell::OsAppearance;
use sx_theme::{Mode, Token};
use sx_win::Drive;

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
    /// The switches: their values, Hot Switch, `settings.toml`.
    pub settings: SettingsStore,
    /// The OS supports system backdrops (Windows 11 22H2+).
    pub backdrop_supported: bool,
    pub appearance: OsAppearance,
    /// `--appearance light|dark`: ignore the Windows app theme for this session.
    pub forced_light: Option<bool>,
    /// `--bench-open`: process start; the first listing prints its timings and quits.
    pub bench_started: Option<Instant>,
    /// Requests forwarded by later launches (single instance).
    pub requests: Option<mpsc::UnboundedReceiver<OpenRequest>>,
    /// Where the operation journal lives (`%LOCALAPPDATA%\SwitchExplorer\journal`).
    pub journal_dir: Option<PathBuf>,
    /// `%APPDATA%\SwitchExplorer\pins.json`; `None` keeps pins for this session only.
    pub pins_file: Option<PathBuf>,
}

/// A pinned sidebar item.
#[derive(Clone, Debug)]
pub struct Pin {
    pub name: SharedString,
    pub path: PathBuf,
}

// The flags are independent UI states (address editing, sidebar, backdrop support, pending
// rename), not a mode that an enum would describe better.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct Workspace {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    next_tab_id: u64,
    pub(crate) list_focus: FocusHandle,
    pub(crate) address: gpui_kit::Entity<InputState>,
    pub(crate) editing_address: bool,
    /// The search box (F-SRCH-01); it shows the active tab's filter.
    pub(crate) search: gpui_kit::Entity<InputState>,
    /// What the search box was last given (tab id, text) and the place its placeholder names;
    /// see `sync_search`.
    pub(crate) search_shown: Option<(u64, String)>,
    pub(crate) search_place: Option<(u64, Location, bool)>,
    /// Generation of the last recursive search started.
    pub(crate) next_search: u64,
    /// `notice` is about a bad search query (cleared with the search box).
    pub(crate) query_notice: bool,
    /// The search box is open although the window is narrow (it folds into an icon there).
    pub(crate) search_open: bool,
    pub(crate) drives: Vec<Drive>,
    pub(crate) pins: Vec<Pin>,
    pins_file: Option<PathBuf>,
    pub(crate) sidebar_visible: bool,
    /// The sidebar item or card under the pointer. Their children (icon, caption, gauge) take
    /// the highlight colours from this; GPUI group hover is not known yet when text is laid out.
    pub(crate) hovered: Option<gpui_kit::ElementId>,
    pub(crate) features: Arc<Features>,
    /// Where `features` comes from; Hot Switch changes it (and writes `settings.toml`).
    pub(crate) settings: SettingsStore,
    pub(crate) settings_writer: crate::hotswitch::SettingsWriter,
    /// The Hot Switch OSD, while it shows.
    pub(crate) osd: Option<crate::hotswitch::Osd>,
    /// The command palette, while it is open.
    pub(crate) palette: Option<crate::palette::Palette>,
    pub(crate) next_osd: u64,
    /// One-line message about the last failed action (cause + fix, DESIGN.md §10).
    pub(crate) notice: Option<SharedString>,
    pub(crate) hwnd: Option<isize>,
    pub(crate) ops: crate::ops::OpsState,
    pub(crate) icons: crate::icons::IconCache,
    pub(crate) dialog_focus: FocusHandle,
    /// The areas `F6` moves between (with `list_focus`), and the chrome's tab stops.
    pub(crate) command_area: FocusHandle,
    pub(crate) sidebar_area: FocusHandle,
    pub(crate) toast_area: FocusHandle,
    pub(crate) focus_stops: crate::focus::FocusStops,
    /// What has focus in the frame being drawn (rings are drawn from it).
    pub(crate) focused_now: Option<FocusHandle>,
    /// Start renaming the selected item on the next render (it needs the window).
    pub(crate) rename_pending: bool,
    /// The details column border being dragged.
    pub(crate) column_resize: Option<crate::details::ColumnResize>,
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
        let search = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&address, window, Self::on_address_event),
            cx.subscribe_in(&search, window, Self::on_search_event),
            cx.on_focus_lost(window, Self::restore_focus),
        ];
        let list_focus = cx.focus_handle().tab_stop(true);
        window.focus(&list_focus, cx);
        let features = options.settings.snapshot();
        let undo_depth = features.ops_undo_depth;
        let (icons, loaded_icons) =
            crate::icons::IconCache::start(usize::from(features.perf_icon_cache_entries));
        let (changes, changed_tabs) = mpsc::unbounded();

        let mut this = Self {
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 1,
            list_focus,
            address,
            editing_address: false,
            search,
            search_shown: None,
            search_place: None,
            next_search: 0,
            query_notice: false,
            search_open: false,
            drives: Vec::new(),
            pins: crate::pins::load(options.pins_file.as_deref())
                .into_iter()
                .map(Pin::new)
                .collect(),
            pins_file: options.pins_file.clone(),
            sidebar_visible: true,
            hovered: None,
            features,
            settings: options.settings,
            settings_writer: crate::hotswitch::SettingsWriter::start(),
            osd: None,
            palette: None,
            next_osd: 0,
            notice: None,
            hwnd: hwnd(window),
            ops: crate::ops::OpsState::new(options.journal_dir.as_deref(), usize::from(undo_depth)),
            dialog_focus: cx.focus_handle(),
            command_area: cx.focus_handle(),
            sidebar_area: cx.focus_handle(),
            toast_area: cx.focus_handle(),
            focus_stops: crate::focus::FocusStops::default(),
            focused_now: None,
            icons,
            rename_pending: false,
            column_resize: None,
            backdrop_supported: options.backdrop_supported,
            appearance: options.appearance,
            forced_light: options.forced_light,
            bench_started: options.bench_started,
            bench_report: None,
            changes,
            pending_refresh: HashSet::new(),
            _subscriptions: subscriptions,
        };
        if this.features.ops_taskbar_progress {
            this.ops.taskbar = this.hwnd.and_then(|hwnd| {
                sx_shell::taskbar::Taskbar::new(hwnd)
                    .map_err(|err| tracing::debug!(error = %err, "taskbar progress unavailable"))
                    .ok()
            });
        }
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
        this.receive_icons(loaded_icons, cx);
        if let Some(requests) = options.requests {
            this.watch_requests(requests, window, cx);
        }
        this.sync_window(window);
        this.report_interrupted();
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

    pub(crate) fn tab_by_id_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
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
        // Refreshing search results (F5, a finished operation, a view switch) searches again,
        // with the view switches in effect; the listing behind them is reloaded afterwards.
        if refresh && tab.search.is_some() {
            tab.sort = SortSpec {
                folders_first: features.view_folders_first,
                natural: features.view_natural_sort,
                ..tab.sort
            };
            if let Some(search) = &mut tab.search {
                search.listing_stale = true;
            }
            let text = tab.filter.clone();
            self.start_search(tab_id, &text, true, cx);
            return;
        }
        let refresh = refresh && matches!(tab.content, Content::Folder(_));
        if !refresh {
            tab.watcher = None;
            // Another folder: the filter and the search were for the old one.
            tab.filter.clear();
            tab.recursive = false;
            tab.take_search();
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
            filter: tab
                .listing_filter(features.search_recursive_default)
                .to_string(),
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
        // A listing that arrives while results are shown waits behind them. (It may be older
        // than a change the watcher saw, so the listing stays stale if it was.)
        if let Some(search) = &mut tab.search {
            if let Ok(snapshot) = result {
                search.listing = Some(Arc::new(snapshot));
            }
            return;
        }
        match result {
            Ok(mut snapshot) => {
                let filter = tab.listing_filter(self.features.search_recursive_default);
                if snapshot.view().filter != sx_fs::fold_case(filter) {
                    let view = ViewOptions {
                        filter: filter.to_string(),
                        ..snapshot.view().clone()
                    };
                    snapshot.set_view(view);
                }
                tracing::debug!(
                    entries = snapshot.len(),
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    "folder loaded"
                );
                let entries = snapshot.len();
                if refresh {
                    restore_selection(tab, snapshot);
                    if reveal_pending(tab) {
                        self.rename_pending = true;
                    }
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    if watch
                        && !tab
                            .watcher
                            .as_ref()
                            .is_some_and(sx_win::DirWatcher::is_running)
                    {
                        tab.watcher =
                            sx_win::watch(path, move || changes.unbounded_send(tab_id).is_ok())
                                .map_err(
                                    |err| tracing::debug!(error = %err, "folder watch unavailable"),
                                )
                                .ok();
                    }
                    cx.notify();
                    return;
                }
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
                if watch
                    && !tab
                        .watcher
                        .as_ref()
                        .is_some_and(sx_win::DirWatcher::is_running)
                {
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
                // The folder is gone or unreadable: stop holding a handle on it (and its drive).
                tab.watcher = None;
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

    /// Icons from the worker land in the cache; the rows that wanted them redraw.
    fn receive_icons(
        &mut self,
        mut loaded: mpsc::UnboundedReceiver<(
            sx_shell::icons::IconKey,
            Option<sx_shell::icons::IconBitmap>,
        )>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some((key, icon)) = loaded.next().await {
                let alive = this.update(cx, |this, cx| {
                    this.icons.insert(key, icon);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
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
                        let tab = this.tabs.iter_mut().find(|t| t.id == tab_id);
                        if let Some(search) = tab.and_then(|t| t.search.as_mut()) {
                            // A change in the folder must not replace the results on screen.
                            search.listing_stale = true;
                        } else {
                            this.load_tab(tab_id, true, cx);
                        }
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
            // Tab moved focus on to the next control: leave it there. Enter and Escape leave
            // it on the field, which is about to go: the list takes it.
            let field = self.address.read(cx).focus_handle(cx);
            if window.focused(cx).is_none_or(|focused| focused == field) {
                window.focus(&self.list_focus, cx);
            }
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
                    Some(Address::Shell(name)) => {
                        self.leave_address(window, cx);
                        let resolving = sx_shell::names::resolve(name.clone());
                        cx.spawn(async move |this, cx| {
                            let folder = resolving.await;
                            let _ = this.update(cx, |this, cx| {
                                if let Some(path) = folder {
                                    this.navigate(Location::Path(path), cx);
                                } else {
                                    this.notice = Some(t!("nav.shell_unknown", name = name).into());
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
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
        tab.sort = SortSpec {
            column,
            descending: tab.sort.column == column && !tab.sort.descending,
            ..tab.sort
        };
        // Results still coming in are sorted once the walk ends, in this order.
        if tab.search.as_ref().is_some_and(|s| s.running) {
            cx.notify();
            return;
        }
        let tab_id = tab.id;
        self.resort(tab_id, cx);
    }

    /// Sorts a tab's listing by its sort spec on a background executor; the selection is kept.
    pub(crate) fn resort(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let spec = tab.sort;
        let Some(snapshot) = tab.snapshot().cloned() else {
            cx.notify();
            return;
        };
        // The order is for this very listing; one that replaced it meanwhile is left alone.
        let sorted_listing = Arc::as_ptr(&snapshot) as usize;
        let task = cx
            .background_executor()
            .spawn(async move { snapshot.sorted_order(spec) });
        cx.spawn(async move |this, cx| {
            let order = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) else {
                    return;
                };
                if tab.snapshot().map(|s| Arc::as_ptr(s) as usize) != Some(sorted_listing) {
                    return;
                }
                let focus_row = tab.change_listing(|snapshot| snapshot.apply_order(spec, order));
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

/// After a refresh: selects an item that was just renamed or created. Returns `true` when the
/// new item should go into rename mode.
fn reveal_pending(tab: &mut Tab) -> bool {
    let find = |tab: &Tab, name: &str| {
        let snapshot = tab.snapshot()?;
        (0..snapshot.visible_len()).find(|&r| snapshot.row(r).is_some_and(|e| e.name == name))
    };
    let mut rename = false;
    for (name, start_rename) in [
        (tab.rename_after_load.clone(), true),
        (tab.reveal_after_refresh.clone(), false),
    ] {
        let Some(name) = name else { continue };
        if let Some(row) = find(tab, &name) {
            tab.selection.select_only(row as u32);
            tab.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            if start_rename {
                tab.rename_after_load = None;
                rename = true;
            } else {
                tab.reveal_after_refresh = None;
            }
        }
    }
    rename
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

impl Pin {
    fn new(path: PathBuf) -> Self {
        Self {
            name: path
                .file_name()
                .map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
                .into(),
            path,
        }
    }
}

impl Workspace {
    pub(crate) fn is_pinned(&self, path: &std::path::Path) -> bool {
        self.pins.iter().any(|p| p.path == path)
    }

    /// Pins or unpins the current folder (F-SIDE-02), unless `sidebar.pins_locked`.
    pub(crate) fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.tab().location().as_path().map(PathBuf::from) else {
            return;
        };
        if self.features.sidebar_pins_locked {
            self.notice = Some(t!("nav.pins_locked").into());
            cx.notify();
            return;
        }
        if self.is_pinned(&path) {
            self.pins.retain(|p| p.path != path);
        } else {
            self.pins.push(Pin::new(path));
        }
        if let Some(file) = &self.pins_file {
            let paths: Vec<PathBuf> = self.pins.iter().map(|p| p.path.clone()).collect();
            if let Err(err) = crate::pins::save(file, &paths) {
                tracing::warn!(error = %err, "pins not saved");
            }
        }
        cx.notify();
    }
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
        self.focus_stops.begin_frame();
        // A dialog (a paste conflict) or an inline rename that starts takes over from the
        // palette; the focus goes to it.
        if self.palette.is_some() && (self.ops.dialog.is_some() || self.ops.renaming.is_some()) {
            self.palette = None;
        }
        self.sync_search(window, cx);
        self.focused_now = window.focused(cx);
        // The toast holds its timer while it has focus (keyboard users reaching for Undo).
        self.ops.toast_focused = self.toast_area.contains_focused(window, cx);
        if std::mem::take(&mut self.rename_pending) {
            self.begin_rename(window, cx);
        }
        // A closed dialog leaves focus on an element that is gone; give it back to the list so
        // shortcuts (Ctrl+Z right after confirming) keep working.
        if self.ops.dialog.is_none() && self.dialog_focus.is_focused(window) {
            window.focus(&self.list_focus, cx);
        }
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
        let overlays = self.render_overlays(window, cx);
        let root = div()
            .id("workspace")
            .key_context(WORKSPACE_CONTEXT)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(font)
            .text_color(fg)
            .text_size(size)
            .line_height(line)
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
            .children(overlays)
            .children(self.render_palette(window, cx))
            // Above dialogs too: a Hot Switch is never dimmed by a scrim.
            .children(self.render_osd(cx));
        let root = self.nav_actions(root, cx);
        let root = self.file_actions(root, cx);
        let root = self.hotswitch_actions(root, cx);
        self.focus_actions(root, cx)
    }
}

impl Workspace {
    /// Navigation and search commands, bound from the `CommandRegistry`.
    fn nav_actions(
        &self,
        root: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        root.on_action(cx.listener(|this, _: &actions::NavBack, _, cx| {
            // Results have no place in the history: Back goes back to the folder listing.
            if this.tab().search.is_some() {
                let tab_id = this.tab().id;
                this.end_search(tab_id, false, cx);
                return;
            }
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
        .on_action(cx.listener(|this, _: &actions::SearchFilter, window, cx| {
            this.focus_search(window, cx);
        }))
        .on_action(cx.listener(|this, _: &actions::SearchFind, window, cx| {
            this.find(window, cx);
        }))
        .on_action(
            cx.listener(|this, _: &actions::CommandPalette, window, cx| {
                this.open_palette(window, cx);
            }),
        )
    }
}

impl Workspace {
    /// File operation commands (F-OPS), bound from the `CommandRegistry`.
    fn file_actions(
        &self,
        root: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        root.on_action(cx.listener(|this, _: &actions::FileDelete, _, cx| {
            this.delete_selection(false, cx);
        }))
        .on_action(
            cx.listener(|this, _: &actions::FileDeletePermanent, _, cx| {
                this.delete_selection(true, cx);
            }),
        )
        .on_action(cx.listener(|this, _: &actions::FileRename, window, cx| {
            this.begin_rename(window, cx);
        }))
        .on_action(cx.listener(|this, _: &actions::FileNewFolder, _, cx| this.new_folder(cx)))
        .on_action(cx.listener(|this, _: &actions::FileCopy, _, cx| this.copy_selection(false, cx)))
        .on_action(cx.listener(|this, _: &actions::FileCut, _, cx| this.copy_selection(true, cx)))
        .on_action(cx.listener(|this, _: &actions::FilePaste, _, cx| this.paste(cx)))
        .on_action(cx.listener(|this, _: &actions::FileCopyPath, _, cx| this.copy_paths(cx)))
        .on_action(cx.listener(|this, _: &actions::EditUndo, _, cx| this.undo(cx)))
        .on_action(cx.listener(|this, _: &actions::EditRedo, _, cx| this.redo(cx)))
        .on_action(cx.listener(|this, _: &actions::SidebarPin, _, cx| this.toggle_pin(cx)))
        .on_action(cx.listener(|this, _: &actions::TransferPanel, _, cx| {
            this.ops.transfer_panel = !this.ops.transfer_panel;
            cx.notify();
        }))
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
