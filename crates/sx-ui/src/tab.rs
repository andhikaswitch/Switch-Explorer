//! One tab: its history, the listing it shows, selection and scroll state.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
    }

    /// Shows a new listing, keeping the selection by entry name when it is the same folder.
    pub fn set_snapshot(&mut self, snapshot: Arc<DirSnapshot>) {
        self.rows.borrow_mut().clear();
        self.content = Content::Folder(snapshot);
    }

    pub fn clear_row_cache(&self) {
        self.rows.borrow_mut().clear();
    }

    /// Display strings of a visible row, formatted once and cached.
    pub fn row_text(&self, row: usize) -> Option<RowText> {
        let snapshot = self.snapshot()?;
        let entry = snapshot.row(row)?;
        let mut rows = self.rows.borrow_mut();
        if rows.len() > ROW_CACHE_LIMIT {
            rows.clear();
        }
        Some(
            rows.entry(entry.index)
                .or_insert_with(|| format::row_text(&entry))
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
