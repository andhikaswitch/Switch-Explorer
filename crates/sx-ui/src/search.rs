//! The search box. Typing filters the current folder as you type (F-SRCH-01), and the matching
//! part of each name shows in bold `match.fg`. `Enter` searches the folder and its subfolders
//! (F-SRCH-02): results stream in with a Folder column, and while results are shown, typing
//! searches again after a short pause (the running search stops at once). `Ctrl+F` focuses the
//! box; `Ctrl+E` / `F3` focus it for searching subfolders straight away. `Escape` clears it and
//! goes back to the list; `Down` goes to the list. The box belongs to the tab: going to another
//! folder clears it. In a narrow window (DESIGN.md §8.8) the box is an icon until it is used.

use std::collections::HashSet;
use std::ops::{ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt as _;
use futures::channel::mpsc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, Entity, Focusable as _, FontWeight, HighlightStyle,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledText, Window, div,
};
use rust_i18n::t;
use sx_fs::{Column, DirSnapshot, ViewOptions};
use sx_search::{Query, Stats, WalkOptions};
use sx_theme::Token;
use sx_win::RawEntry;

use crate::actions::WORKSPACE_CONTEXT;
use crate::focus::TabStop as _;
use crate::tab::{Content, SearchState};
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// A recursive search stops at this many results; the status bar says so.
pub(crate) const RESULT_LIMIT: usize = 100_000;

/// In recursive mode a new search starts once typing pauses this long.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// What the search thread sends: the matches of one folder, or the end of the walk.
enum Found {
    Folder(PathBuf, Vec<RawEntry>),
    Done { stats: Stats, capped: bool },
}

/// Below this window width the box folds into an icon (DESIGN.md §8.8): with the window at its
/// 560 px minimum the address bar would otherwise be squeezed to nothing.
const COLLAPSE_BELOW: f32 = 720.0;

impl Workspace {
    pub(crate) fn render_search_box(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        let narrow = f32::from(window.viewport_size().width) < COLLAPSE_BELOW;
        if narrow && !focused && !self.search_open && self.tab().filter.is_empty() {
            return self.render_search_icon(cx);
        }
        let t = cx.tokens();
        let hover = t.color(Token::BorderInputHover);
        let radius = t.px(Token::RadiusSm);
        div()
            .id("search-box")
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .ml(t.px(Token::SpaceSm))
            .w(t.px(Token::SizeSearchWidth))
            .h(t.px(Token::SizeControl))
            .px(t.px(Token::SpaceXs))
            .rounded(radius)
            .bg(t.paint(Token::SurfaceInput))
            .border_1()
            .map(|el| {
                if focused {
                    el.border_color(t.color(Token::BorderInputFocus))
                        .child(crate::focus::ring(radius, cx))
                } else {
                    el.border_color(t.color(Token::BorderInput))
                        .hover(move |s| s.border_color(hover))
                }
            })
            .capture_key_down(cx.listener(Self::search_key_down))
            .child(
                Input::new(&self.search)
                    .appearance(false)
                    // A fixed name: the placeholder changes with the folder.
                    .aria_label(SharedString::from(t!("search.label")))
                    .prefix(
                        Icon::new(IconName::Search)
                            .size(t.px(Token::SizeIconSm))
                            .text_color(t.color(Token::FgMuted)),
                    )
                    .when(!self.tab().filter.is_empty(), |input| {
                        input.suffix(self.render_search_clear(cx))
                    })
                    .h_full(),
            )
            .into_any_element()
    }

    /// The box folded into an icon button; clicking it (or `Ctrl+F`) opens the box.
    fn render_search_icon(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let (hover, pressed, hover_fg) = (
            t.paint(Token::ButtonGhostBgHover),
            t.paint(Token::ButtonGhostBgPressed),
            t.color(Token::ButtonGhostFgHover),
        );
        let focus = self.focus_stop(&ElementId::Name("search-open".into()), cx);
        div()
            .id("search-open")
            .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .ml(t.px(Token::SpaceSm))
            .size(t.px(Token::SizeControl))
            .rounded(t.px(Token::RadiusSm))
            .text_color(t.color(Token::ButtonGhostFg))
            .hover(move |s| s.bg(hover).text_color(hover_fg))
            .active(move |s| s.bg(pressed).text_color(hover_fg))
            .tooltip(|window, cx| {
                Tooltip::new(SharedString::from(t!("search.label")))
                    .action(&crate::actions::SearchFilter, Some(WORKSPACE_CONTEXT))
                    .build(window, cx)
            })
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.focus_search(window, cx);
            }))
            .child(Icon::new(IconName::Search).size(t.px(Token::SizeIconSm)))
            .into_any_element()
    }

    /// The box's clear button (24 px target, DESIGN.md §14), shown while it holds text.
    fn render_search_clear(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let (hover, hover_fg) = (
            t.paint(Token::ButtonGhostBgHover),
            t.color(Token::ButtonGhostFgHover),
        );
        let icon = t.px(Token::SizeIconSm);
        div()
            .id("search-clear-button")
            .role(gpui_kit::Role::Button)
            .aria_label(SharedString::from(t!("search.clear")))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(icon + t.px(Token::SpaceSm))
            .rounded(t.px(Token::RadiusXs))
            .text_color(t.color(Token::FgSecondary))
            .hover(move |s| s.bg(hover).text_color(hover_fg))
            .tooltip(|window, cx| {
                Tooltip::new(SharedString::from(t!("search.clear_tip"))).build(window, cx)
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.clear_search(cx);
            }))
            .child(Icon::new(IconName::Close).size(icon * 0.75))
    }

    pub(crate) fn on_search_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let text = input.read(cx).value().to_string();
                self.search_shown = Some((self.tab().id, text.clone()));
                self.set_filter(&text, cx);
            }
            InputEvent::PressEnter { .. } => {
                let text = input.read(cx).value().to_string();
                let tab_id = self.tab().id;
                self.start_search(tab_id, &text, false, cx);
            }
            InputEvent::Blur => {
                self.search_open = false;
                cx.notify();
            }
            InputEvent::Focus => cx.notify(),
        }
    }

    fn search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = event.keystroke.modifiers;
        if m.control || m.alt || m.shift || m.platform {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.clear_search(cx);
                window.focus(&self.list_focus, cx);
            }
            "down" => {
                cx.stop_propagation();
                self.search_to_list(window, cx);
            }
            _ => {}
        }
    }

    /// `Ctrl+F`: into the search box (opened if it is folded), with what it holds selected.
    pub(crate) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().location().as_path().is_none() {
            return;
        }
        self.search_open = true;
        self.search.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// From the box to the list: the first row gets selected if nothing is.
    fn search_to_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.list_focus, cx);
        if self.tab().selection.is_empty() && self.tab().row_count() > 0 {
            self.select_row(0, false, false);
        }
        cx.notify();
    }

    /// `Ctrl+E` / `F3`: into the search box for searching subfolders too; what it holds is
    /// searched at once.
    pub(crate) fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().location().as_path().is_none() {
            return;
        }
        self.tab_mut().recursive = true;
        self.focus_search(window, cx);
        let (tab_id, text) = (self.tab().id, self.tab().filter.clone());
        if !text.trim().is_empty() && self.tab().search.is_none() {
            self.start_search(tab_id, &text, false, cx);
        }
    }

    /// Whether typing in the box searches subfolders (results shown, `Ctrl+E`, or
    /// `search.recursive_default`) rather than filtering the folder.
    pub(crate) fn typing_searches(&self) -> bool {
        self.tab().searches(self.features.search_recursive_default)
    }

    /// `Esc` or the × button: the box empties; a recursive search ends, and its mode with it.
    pub(crate) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if self.tab().search.is_some() || self.tab().recursive {
            let tab_id = self.tab().id;
            self.end_search(tab_id, false, cx);
        } else {
            self.set_filter("", cx);
        }
        self.clear_query_notice();
        cx.notify();
    }

    /// What the box holds changed: filters the folder, or (recursive mode) searches again once
    /// typing pauses. A box emptied by typing ends the search but stays in recursive mode. The
    /// selection stays on the entries still shown.
    pub(crate) fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.tab().filter == text {
            return;
        }
        if self.typing_searches() {
            let tab_id = self.tab().id;
            self.tab_mut().filter = text.to_string();
            if text.trim().is_empty() {
                self.end_search(tab_id, true, cx);
            } else {
                self.search_later(tab_id, cx);
            }
            cx.notify();
            return;
        }
        let tab = self.tab_mut();
        tab.filter = text.to_string();
        let started = std::time::Instant::now();
        let focus_row = tab.change_listing(|snapshot| {
            let view = ViewOptions {
                filter: text.to_string(),
                ..snapshot.view().clone()
            };
            snapshot.set_view(view);
        });
        // The cached row text holds the match to highlight.
        tab.clear_row_cache();
        tracing::trace!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            rows = tab.row_count(),
            "filter applied"
        );
        match focus_row {
            Some(row) => tab
                .scroll
                .scroll_to_item(row as usize, ScrollStrategy::Nearest),
            None => tab.scroll.scroll_to_item(0, ScrollStrategy::Top),
        }
        cx.notify();
    }

    /// Keeps the box in step with the active tab: its text and "Search <folder>" (another tab,
    /// a folder change, a search ended from elsewhere). Called at the start of a frame; it
    /// compares what the box was last given, so a frame with nothing new costs no allocation.
    pub(crate) fn sync_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let value_differs = self
            .search_shown
            .as_ref()
            .is_none_or(|(id, text)| *id != tab.id || *text != tab.filter);
        let recursive = self.typing_searches();
        let place_differs = self
            .search_place
            .as_ref()
            .is_none_or(|(id, location, deep)| {
                *id != tab.id || location != tab.location() || *deep != recursive
            });
        if !value_differs && !place_differs {
            return;
        }
        let value = value_differs.then(|| tab.filter.clone());
        let placeholder: Option<SharedString> = place_differs.then(|| {
            if recursive {
                t!("search.placeholder_recursive", folder = tab.title()).into()
            } else {
                t!("search.placeholder", folder = tab.title()).into()
            }
        });
        let (id, location) = (tab.id, tab.location().clone());
        if let Some(value) = &value {
            self.search_shown = Some((id, value.clone()));
        }
        if place_differs {
            self.search_place = Some((id, location, recursive));
        }
        self.search.update(cx, |input, cx| {
            if let Some(value) = value {
                input.set_value(value, window, cx);
            }
            if let Some(placeholder) = placeholder {
                input.set_placeholder(placeholder, window, cx);
            }
        });
    }

    fn clear_query_notice(&mut self) {
        if std::mem::take(&mut self.query_notice) {
            self.notice = None;
        }
    }
}

impl Workspace {
    /// Searches a tab's folder and its subfolders for `text` (F-SRCH-02).
    ///
    /// A new query shows its results as they are found. A `refresh` (F5, a finished operation,
    /// a view switch) keeps the results on screen until the new walk ends, then swaps them in
    /// with the selection kept by path. A query with a bad operator value says what is wrong
    /// instead of searching.
    pub(crate) fn start_search(
        &mut self,
        tab_id: u64,
        text: &str,
        refresh: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        // A search pending after typing is overtaken by this one.
        tab.search_ticket += 1;
        let Some(root) = tab.location().as_path().map(Path::to_path_buf) else {
            return;
        };
        let query = match Query::parse(text) {
            Ok(query) if query.is_empty() => {
                if !refresh {
                    self.end_search(tab_id, true, cx);
                }
                return;
            }
            Ok(query) => query,
            Err(err) => {
                if !refresh {
                    self.notice = Some(crate::format::query_error(&err));
                    self.query_notice = true;
                    cx.notify();
                }
                return;
            }
        };
        if !refresh {
            self.clear_query_notice();
        }
        self.next_search += 1;
        let generation = self.next_search;
        let options = WalkOptions {
            show_hidden: self.features.view_show_hidden,
            show_system: self.features.view_show_system,
        };
        let view = ViewOptions {
            show_hidden: options.show_hidden,
            show_system: options.show_system,
            filter: String::new(),
        };
        let highlight = query
            .words()
            .next()
            .map(sx_fs::fold_case)
            .unwrap_or_default();
        // The thread first: if it cannot start, nothing on screen has changed.
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded();
        let walk = std::thread::Builder::new().name("sx-search".into()).spawn({
            let cancel = cancel.clone();
            move || run_search(&root, &query, options, &cancel, &tx)
        });
        if let Err(err) = walk {
            tracing::warn!(error = %err, "search thread not started");
            if !refresh {
                self.notice = Some(t!("search.failed").into());
            }
            cx.notify();
            return;
        }
        // An inline rename follows an entry index, which the new results reuse.
        if self
            .ops
            .renaming
            .as_ref()
            .is_some_and(|r| r.tab_id == tab_id)
        {
            self.ops.renaming = None;
        }
        let search = SearchState {
            generation,
            cancel,
            running: true,
            highlight,
            stats: None,
            capped: false,
            listing: None,
            listing_stale: false,
            building: None,
        };
        self.install_search(tab_id, text, refresh, search, view);
        self.receive_found(rx, tab_id, generation, cx);
        cx.notify();
    }

    /// Puts a starting search in its tab: new results replace what is shown (a refresh keeps
    /// it until the new results are in), and the folder listing waits to come back.
    fn install_search(
        &mut self,
        tab_id: u64,
        text: &str,
        refresh: bool,
        mut search: SearchState,
        view: ViewOptions,
    ) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        tab.filter = text.to_string();
        let replacing = tab.take_search();
        let keep_on_screen = refresh && replacing.is_some();
        // The listing to go back to: kept from a search being replaced, else the one shown.
        let (listing, listing_stale) = match replacing {
            Some(old) => (old.listing, old.listing_stale),
            None => match &tab.content {
                Content::Folder(snapshot) if !snapshot.is_results() => {
                    (Some(snapshot.clone()), false)
                }
                _ => (None, false),
            },
        };
        let fresh = DirSnapshot::new(tab.sort, view);
        if keep_on_screen {
            search.building = Some(fresh);
        } else {
            tab.set_snapshot(Arc::new(fresh));
            tab.selection.clear();
            tab.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        search.listing = listing;
        search.listing_stale = listing_stale;
        tab.search = Some(search);
    }

    /// Feeds what the search thread sends into the tab, in one update per burst.
    fn receive_found(
        &self,
        mut rx: mpsc::UnboundedReceiver<Found>,
        tab_id: u64,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(first) = rx.next().await {
                // Everything that arrived meanwhile goes in with one update.
                let mut found = vec![first];
                while let Ok(more) = rx.try_recv() {
                    found.push(more);
                }
                let alive =
                    this.update(cx, |this, cx| this.on_found(tab_id, generation, found, cx));
                if alive.is_err() {
                    return;
                }
            }
            // The thread is gone: finished, or stopped without saying so.
            let _ = this.update(cx, |this, cx| this.walk_ended(tab_id, generation, cx));
        })
        .detach();
    }

    /// Recursive mode: stops the running search now and searches again once typing pauses.
    /// The results stay on screen meanwhile, marked as being searched.
    fn search_later(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        self.next_search += 1;
        let pending = self.next_search;
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        if let Some(search) = &mut tab.search {
            search.cancel.store(true, Ordering::Relaxed);
            // What the stopped walk still sends belongs to no search.
            search.generation = pending;
            search.running = true;
            search.building = None;
        }
        tab.search_ticket += 1;
        let ticket = tab.search_ticket;
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tab_by_id_mut(tab_id) else {
                    return;
                };
                if tab.search_ticket == ticket {
                    let text = tab.filter.clone();
                    this.start_search(tab_id, &text, false, cx);
                }
            });
        })
        .detach();
    }

    fn on_found(
        &mut self,
        tab_id: u64,
        generation: u64,
        found: Vec<Found>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        let Some(search) = tab.search.as_mut().filter(|s| s.generation == generation) else {
            return;
        };
        let mut done = false;
        for message in found {
            match message {
                Found::Folder(folder, entries) => match &mut search.building {
                    Some(next) => next.push_found(&folder, &entries),
                    None => {
                        if let Content::Folder(results) = &mut tab.content {
                            Arc::make_mut(results).push_found(&folder, &entries);
                        }
                    }
                },
                Found::Done { stats, capped } => {
                    search.stats = Some(stats);
                    search.capped = capped;
                    done = true;
                }
            }
        }
        if done {
            self.finish_search(tab_id, generation, cx);
        }
        cx.notify();
    }

    /// The walk ended: the results are sorted by the tab's order (off the UI thread); a
    /// refresh's new results replace the ones shown.
    fn finish_search(&mut self, tab_id: u64, generation: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        let spec = tab.sort;
        let Some(search) = tab.search.as_mut().filter(|s| s.generation == generation) else {
            return;
        };
        let Some(mut next) = search.building.take() else {
            search.running = false;
            self.resort(tab_id, cx);
            return;
        };
        let sorted = cx.background_executor().spawn(async move {
            next.sort(spec);
            next
        });
        cx.spawn(async move |this, cx| {
            let next = sorted.await;
            let _ = this.update(cx, |this, cx| {
                this.swap_results(tab_id, generation, next, cx);
            });
        })
        .detach();
    }

    /// Installs a refresh's results, keeping the selected and focused items by path.
    fn swap_results(
        &mut self,
        tab_id: u64,
        generation: u64,
        next: DirSnapshot,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        let Some(search) = tab.search.as_mut().filter(|s| s.generation == generation) else {
            return;
        };
        search.running = false;
        let (rows, focus) = match tab.snapshot() {
            Some(old) => keep_by_path(old, &tab.selection, &next),
            None => (Vec::new(), None),
        };
        tab.set_snapshot(Arc::new(next));
        tab.selection.restore(rows, focus);
        cx.notify();
    }

    /// The search thread is gone. If it never said it was done (it panicked), the search is
    /// over all the same, so "Searching…" does not stay forever.
    fn walk_ended(&mut self, tab_id: u64, generation: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        if let Some(search) = tab.search.as_mut().filter(|s| s.generation == generation)
            && search.stats.is_none()
        {
            tracing::warn!("search ended without a result");
            search.running = false;
            search.building = None;
            cx.notify();
        }
    }

    /// Ends a tab's recursive search: the folder listing comes back unfiltered, in the tab's
    /// order (reloaded if the folder changed meanwhile). `keep_mode` (the box emptied by
    /// typing) leaves the box and recursive mode as they are.
    pub(crate) fn end_search(&mut self, tab_id: u64, keep_mode: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_by_id_mut(tab_id) else {
            return;
        };
        tab.search_ticket += 1;
        if !keep_mode {
            tab.recursive = false;
            tab.filter.clear();
        }
        let Some(search) = tab.take_search() else {
            cx.notify();
            return;
        };
        // A listing has no Folder column to sort by.
        if tab.sort.column == Column::Folder {
            tab.sort.column = Column::Name;
            tab.sort.descending = false;
        }
        match search.listing {
            Some(listing) => {
                let reorder = listing.sort_spec() != tab.sort;
                tab.set_snapshot(listing);
                tab.selection.clear();
                tab.change_listing(|snapshot| {
                    let view = ViewOptions {
                        filter: String::new(),
                        ..snapshot.view().clone()
                    };
                    snapshot.set_view(view);
                });
                tab.scroll.scroll_to_item(0, ScrollStrategy::Top);
                if search.listing_stale {
                    self.load_tab(tab_id, true, cx);
                } else if reorder {
                    self.resort(tab_id, cx);
                }
            }
            None => self.load_tab(tab_id, false, cx),
        }
        cx.notify();
    }
}

/// The rows of `next` holding the items selected (and focused) in `old`, matched by folder
/// and name. Only the selected names are looked up, so a large result set costs no paths.
fn keep_by_path(
    old: &DirSnapshot,
    selection: &sx_core::Selection,
    next: &DirSnapshot,
) -> (Vec<u32>, Option<u32>) {
    let key = |snapshot: &DirSnapshot, row: usize| {
        let entry = snapshot.row(row)?;
        Some((
            snapshot.folder(entry.index)?.to_path_buf(),
            entry.name.to_string(),
        ))
    };
    let selected: HashSet<(PathBuf, String)> = selection
        .iter()
        .filter_map(|row| key(old, row as usize))
        .collect();
    let focus = selection.focus().and_then(|row| key(old, row as usize));
    if selected.is_empty() && focus.is_none() {
        return (Vec::new(), None);
    }
    let names: HashSet<&str> = selected
        .iter()
        .chain(focus.iter())
        .map(|(_, name)| name.as_str())
        .collect();
    let mut rows = Vec::new();
    let mut focus_row = None;
    for row in 0..next.visible_len() {
        let Some(entry) = next.row(row) else {
            continue;
        };
        if !names.contains(entry.name) {
            continue;
        }
        let Some(folder) = next.folder(entry.index) else {
            continue;
        };
        let key = (folder.to_path_buf(), entry.name.to_string());
        if selected.contains(&key) {
            rows.push(row as u32);
        }
        if focus.as_ref() == Some(&key) {
            focus_row = Some(row as u32);
        }
    }
    (rows, focus_row)
}

/// The search thread: walks `root` and sends each folder's matches, then the end. Stops at
/// [`RESULT_LIMIT`], when cancelled, or when the window is gone.
fn run_search(
    root: &Path,
    query: &Query,
    options: WalkOptions,
    cancel: &AtomicBool,
    tx: &mpsc::UnboundedSender<Found>,
) {
    let mut total = 0;
    let mut capped = false;
    let stats = sx_search::search(
        root,
        query,
        options,
        filetime_now(),
        cancel,
        |folder, entries| {
            let take = entries.len().min(RESULT_LIMIT - total);
            total += take;
            capped = total >= RESULT_LIMIT;
            let sent = tx.unbounded_send(Found::Folder(
                folder.to_path_buf(),
                entries[..take].to_vec(),
            ));
            if sent.is_err() || capped {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );
    let _ = tx.unbounded_send(Found::Done { stats, capped });
}

/// The current time as FILETIME ticks (100 ns since 1601), for `modified:`.
fn filetime_now() -> i64 {
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = i64::try_from(since.as_nanos() / 100).unwrap_or(i64::MAX - UNIX_EPOCH_TICKS);
    UNIX_EPOCH_TICKS.saturating_add(ticks)
}

/// Where `folded_filter` (from [`sx_fs::fold_case`]) occurs in `name`, ignoring case.
pub(crate) fn match_range(name: &str, folded_filter: &str) -> Option<Range<usize>> {
    if folded_filter.is_empty() {
        return None;
    }
    let at = sx_fs::fold_case(name).find(folded_filter)?;
    Some(at..at + folded_filter.len())
}

/// A name with its match (from [`match_range`]) in bold `match.fg`.
pub(crate) fn highlighted(
    name: SharedString,
    matched: Option<Range<usize>>,
    color: Hsla,
) -> StyledText {
    let text = StyledText::new(name);
    match matched {
        Some(range) => text.with_highlights([(
            range,
            HighlightStyle {
                color: Some(color),
                font_weight: Some(FontWeight::BOLD),
                ..HighlightStyle::default()
            },
        )]),
        None => text,
    }
}

/// The query as quoted in a message, cut to a readable length.
pub(crate) fn quoted_query(query: &str) -> String {
    const MAX_CHARS: usize = 40;
    if query.chars().count() <= MAX_CHARS {
        return query.to_string();
    }
    let mut cut: String = query.chars().take(MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ignore_case_and_keep_byte_offsets() {
        let folded = sx_fs::fold_case("REP");
        assert_eq!(match_range("Annual report.pdf", &folded), Some(7..10));
        assert_eq!(match_range("Annual report.pdf", ""), None);
        assert_eq!(match_range("Budget.xlsx", &folded), None);
        // Multi-byte characters before and inside the match.
        let name = "Café — ÉMILE.txt";
        let range = match_range(name, &sx_fs::fold_case("émile")).unwrap();
        assert_eq!(&name[range], "ÉMILE");
        // A character whose lowercase has another length is matched as itself.
        let name = "İstanbul";
        let range = match_range(name, &sx_fs::fold_case("İst")).unwrap();
        assert_eq!(&name[range], "İst");
    }

    #[test]
    fn now_is_a_plausible_filetime() {
        // 2024-01-01 is 133_485_408_000_000_000 ticks.
        assert!(filetime_now() > 133_485_408_000_000_000);
    }

    #[test]
    fn long_queries_are_cut_for_messages() {
        assert_eq!(quoted_query("report"), "report");
        let long = "a".repeat(100);
        let cut = quoted_query(&long);
        assert_eq!(cut.chars().count(), 40);
        assert!(cut.ends_with('…'));
    }
}
