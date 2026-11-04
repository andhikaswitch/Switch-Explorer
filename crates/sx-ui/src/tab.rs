//! One tab: its history, the listing it shows, selection and scroll state.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gpui_kit::{SharedString, UniformListScrollHandle};
use rust_i18n::t;
use sx_core::{History, Location, Selection};
use sx_fs::{DirSnapshot, SortSpec};

use crate::format::{self, RowText};

/// Display strings are cached per entry; the cache is dropped when it grows past this.
const ROW_CACHE_LIMIT: usize = 4096;
/// Type-to-select forgets what was typed after this pause (FEATURES.md F-NAV-05).
const TYPE_AHEAD_RESET_MS: u128 = 1000;

#[derive(Debug)]
pub enum Content {
    /// Waiting for the listing. Nothing is drawn for the first 150 ms so fast folders do not
    /// flicker (DESIGN.md §11).
    Loading {
        since: Instant,
    },
    Folder(Arc<DirSnapshot>),
    Failed(SharedString),
    /// Home, This PC and Recycle Bin draw their own page.
    Page,
}

/// A recursive search shown in a tab (F-SRCH-02). While it lasts the tab's content holds the
/// results and the folder listing waits in `listing`.
#[derive(Debug)]
pub struct SearchState {
    /// Which search this is; batches of an older one are dropped.
    pub generation: u64,
    pub cancel: Arc<AtomicBool>,
    pub running: bool,
    /// The first word of the query, case-folded, to highlight in names.
    pub highlight: String,
    pub stats: Option<sx_search::Stats>,
    /// The walk stopped at the result limit.
    pub capped: bool,
    /// The folder listing to go back to.
    pub listing: Option<Arc<DirSnapshot>>,
    /// The folder changed while the results were shown: reload it when the search ends.
    pub listing_stale: bool,
    /// A refresh's results, collected out of sight until its walk ends.
    pub building: Option<DirSnapshot>,
}

#[derive(Debug)]
pub struct Tab {
    pub id: u64,
    pub history: History,
    pub content: Content,
    pub selection: Selection,
    pub scroll: UniformListScrollHandle,
    pub sort: SortSpec,
    cancel: Arc<AtomicBool>,
    rows: RefCell<HashMap<u32, RowText>>,
    /// Entry name to select once the listing arrives (the folder we came up from).
    pub reveal: Option<String>,
    type_ahead: String,
    typed_at: Option<Instant>,
    /// Rows that fit the viewport, measured during the last render (for Page Up/Down).
    pub page_rows: Cell<usize>,
    /// Watches the listed folder (`perf.watcher`); dropped when the tab navigates away.
    pub watcher: Option<sx_win::DirWatcher>,
    /// A folder just created here: select it and start renaming once it is listed.
    pub rename_after_load: Option<String>,
    /// An item just renamed here: select it once it is listed.
    pub reveal_after_refresh: Option<String>,
    /// (selection revision, listing identity) → (selected count, bytes of selected files).
    selection_summary: Cell<Option<(u64, usize, usize, u64)>>,
    /// Widths of the fixed details columns, as dragged in the header.
    pub columns: crate::details::ColumnWidths,
    /// What the search box holds, as typed: the folder's filter (F-SRCH-01), or the query of the
    /// recursive search (F-SRCH-02). Cleared when the tab goes to another folder.
    pub filter: String,
    /// The recursive search the tab shows, if any.
    pub search: Option<SearchState>,
    /// `Ctrl+E` / `F3`: typing searches subfolders too, instead of filtering.
    pub recursive: bool,
    /// Bumped by every change to the box in recursive mode; a debounced search runs only if it
    /// is still the latest.
    pub search_ticket: u64,
}

impl Tab {
    pub fn new(id: u64, location: Location) -> Self {
        Self {
            id,
            history: History::new(location),
            content: Content::Page,
            selection: Selection::default(),
            scroll: UniformListScrollHandle::new(),
            sort: SortSpec::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            rows: RefCell::default(),
            reveal: None,
            type_ahead: String::new(),
            typed_at: None,
            page_rows: Cell::new(10),
            watcher: None,
            rename_after_load: None,
            reveal_after_refresh: None,
            selection_summary: Cell::new(None),
            columns: crate::details::ColumnWidths::default(),
            filter: String::new(),
            search: None,
            recursive: false,
            search_ticket: 0,
        }
    }

    pub fn location(&self) -> &Location {
        self.history.current()
    }

    pub fn title(&self) -> SharedString {
        location_title(self.location())
    }

    pub fn snapshot(&self) -> Option<&Arc<DirSnapshot>> {
        match &self.content {
            Content::Folder(snapshot) => Some(snapshot),
            _ => None,
        }
    }

    /// Visible rows of the current listing (0 for pages and while loading).
    pub fn row_count(&self) -> usize {
        self.snapshot().map_or(0, |s| s.visible_len())
    }

    /// Cancels any listing in flight and returns the flag for the next one.
    pub fn begin_load(&mut self) -> Arc<AtomicBool> {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.cancel.clone()
    }

    pub fn cancel_load(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(search) = &self.search {
            search.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Whether typing in the box searches subfolders rather than filtering the folder.
    pub fn searches(&self, recursive_default: bool) -> bool {
        self.search.is_some() || self.recursive || recursive_default
    }

    /// The filter the folder listing gets: none while the box holds a recursive query.
    pub fn listing_filter(&self, recursive_default: bool) -> &str {
        if self.searches(recursive_default) {
            ""
        } else {
            &self.filter
        }
    }

    /// Stops and forgets the recursive search, if any; returns it (with the folder listing).
    pub fn take_search(&mut self) -> Option<SearchState> {
        let search = self.search.take()?;
        search.cancel.store(true, Ordering::Relaxed);
        Some(search)
    }

    /// Shows a new listing, keeping the selection by entry name when it is the same folder.
    pub fn set_snapshot(&mut self, snapshot: Arc<DirSnapshot>) {
        self.rows.borrow_mut().clear();
        self.content = Content::Folder(snapshot);
    }

    /// Selected count and the bytes of the selected files, computed once per selection change
    /// (the status bar asks every frame; Ctrl+A in a 100k folder must not walk 100k rows each time).
    pub fn selection_summary(&self) -> (usize, u64) {
        let Some(snapshot) = self.snapshot() else {
            return (0, 0);
        };
        let listing = Arc::as_ptr(snapshot) as usize;
        let revision = self.selection.revision();
        if let Some((rev, list, count, bytes)) = self.selection_summary.get()
            && rev == revision
            && list == listing
        {
            return (count, bytes);
        }
        let bytes = self
            .selection
            .iter()
            .filter_map(|row| snapshot.row(row as usize))
            .filter(|e| !e.kind.is_container())
            .map(|e| e.size)
            .sum();
        let count = self.selection.len();
        self.selection_summary
            .set(Some((revision, listing, count, bytes)));
        (count, bytes)
    }

    /// Changes the listing in place (sort order, filter) and keeps the selection and the focus
    /// on the same entries, where they are still shown. Returns the focused row afterwards.
    pub fn change_listing(&mut self, change: impl FnOnce(&mut DirSnapshot)) -> Option<u32> {
        let Content::Folder(snapshot) = &mut self.content else {
            return None;
        };
        let selected: HashSet<u32> = self
            .selection
            .iter()
            .filter_map(|row| snapshot.row(row as usize).map(|e| e.index))
            .collect();
        let focus = self
            .selection
            .focus()
            .and_then(|row| snapshot.row(row as usize).map(|e| e.index));
        change(Arc::make_mut(snapshot));
        let rows: Vec<u32> = if selected.is_empty() {
            Vec::new()
        } else {
            (0..snapshot.visible_len() as u32)
                .filter(|&r| {
                    snapshot
                        .row(r as usize)
                        .is_some_and(|e| selected.contains(&e.index))
                })
                .collect()
        };
        let focus_row = focus.and_then(|ix| snapshot.row_of(ix)).map(|r| r as u32);
        self.selection.restore(rows, focus_row);
        focus_row
    }

    pub fn clear_row_cache(&self) {
        self.rows.borrow_mut().clear();
    }

    /// Display strings of a visible row, formatted once and cached (the cache is cleared when
    /// `view.show_extensions` or the filter changes).
    pub fn row_text(&self, row: usize, show_extensions: bool) -> Option<RowText> {
        let snapshot = self.snapshot()?;
        let entry = snapshot.row(row)?;
        let mut rows = self.rows.borrow_mut();
        if rows.len() > ROW_CACHE_LIMIT {
            rows.clear();
        }
        Some(
            rows.entry(entry.index)
                .or_insert_with(|| {
                    let mut text = format::row_text(&entry, show_extensions);
                    let needle = match &self.search {
                        Some(search) => search.highlight.as_str(),
                        None => snapshot.view().filter.as_str(),
                    };
                    text.matched = crate::search::match_range(&text.name, needle);
                    if let Some(folder) = snapshot.folder(entry.index) {
                        text.folder = format::found_in(self.location().as_path(), folder);
                        text.folder_full = folder.display().to_string().into();
                    }
                    text
                })
                .clone(),
        )
    }

    /// Type-to-select: the visible row whose name starts with what was typed. Typing the same
    /// letter again cycles through the names that start with it, like Explorer.
    pub fn type_ahead(&mut self, ch: char, now: Instant) -> Option<usize> {
        let fresh = self
            .typed_at
            .is_none_or(|at| now.duration_since(at).as_millis() > TYPE_AHEAD_RESET_MS);
        if fresh {
            self.type_ahead.clear();
        }
        self.typed_at = Some(now);
        self.type_ahead.extend(ch.to_lowercase());
        let snapshot = self.snapshot()?;
        let len = snapshot.visible_len();
        if len == 0 {
            return None;
        }
        let query = self.type_ahead.clone();
        let mut chars = query.chars();
        let first = chars.next()?;
        let cycling = chars.all(|c| c == first);
        let needle = if cycling { first.to_string() } else { query };
        let focus = self.selection.focus().map_or(0, |f| f as usize);
        let start = if cycling && !fresh { focus + 1 } else { focus };
        (0..len).map(|offset| (start + offset) % len).find(|&row| {
            snapshot
                .row(row)
                .is_some_and(|e| e.name.to_lowercase().starts_with(&needle))
        })
    }
}

/// Tab and window title of a location.
pub fn location_title(location: &Location) -> SharedString {
    match location {
        Location::Home => t!("nav.home").into(),
        Location::ThisPc => t!("nav.this_pc").into(),
        Location::RecycleBin => t!("nav.recycle_bin").into(),
        Location::Path(path) => path
            .file_name()
            .map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
            .into(),
    }
}
