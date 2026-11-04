//! Context menus (FEATURES.md F-CTX). The shell's own menu (`IContextMenu`, F-CTX-02) opens on a
//! right-click (until the themed menu takes plain right-clicks; `Shift` adds the extended verbs)
//! and with `Shift+F10`, for the selected items or, with nothing selected, for the folder's empty
//! area. The paths are read off the UI thread, then the menu runs on the modal host (ADR-015);
//! Rename, Delete and Paste come back to run here, with the journal and undo, on the items the
//! menu was for.

use std::path::PathBuf;

use gpui_kit::{Context, Pixels, Point, Window};
use rust_i18n::t;
use sx_shell::menu::{MenuOptions, MenuOutcome, MenuTarget, OwnVerb};

use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

impl Workspace {
    /// Starts the inline rename of the item at `path`, if it is listed in the active tab.
    fn rename_path(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let (Some(snapshot), Some(dir)) = (tab.snapshot(), tab.location().as_path()) else {
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy());
        let row = (0..snapshot.visible_len()).find(|&r| {
            snapshot.row(r).is_some_and(|e| {
                name.as_deref() == Some(e.name)
                    && snapshot.path_of(dir, e.index).as_deref() == Some(path)
            })
        });
        if let Some(row) = row {
            self.select_row(row, false, false);
            self.begin_rename(window, cx);
        }
    }

    /// What the shell menu is for: the selected items that share the focused item's folder
    /// (search results can span folders), else the folder itself.
    fn menu_target(&self) -> Option<MenuTarget> {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return self
                .tab()
                .location()
                .as_path()
                .map(|dir| MenuTarget::Background(dir.to_path_buf()));
        }
        let tab = self.tab();
        let focused = tab
            .selection
            .focus()
            .filter(|&row| tab.selection.contains(row))
            .and_then(|row| {
                let snapshot = tab.snapshot()?;
                let entry = snapshot.row(row as usize)?;
                snapshot.path_of(tab.location().as_path()?, entry.index)
            });
        let anchor = focused.unwrap_or_else(|| paths[0].clone());
        let parent = anchor.parent().map(PathBuf::from);
        let items: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| p.parent().map(PathBuf::from) == parent)
            .collect();
        Some(MenuTarget::Items(items))
    }

    /// Where a menu opened from the keyboard goes: on the focused row, when it is on screen.
    pub(crate) fn keyboard_menu_point(&self, cx: &gpui_kit::App) -> Option<Point<Pixels>> {
        let tab = self.tab();
        let row = tab.selection.focus()?;
        let bounds = tab.rows_bounds.get()?;
        let height = cx.tokens().px(sx_theme::Token::RowHeightDefault);
        let offset = tab.scroll.0.borrow().base_handle.offset();
        // Below the row, so its name stays readable.
        let y = bounds.top() + offset.y + height * (row as f32 + 1.0);
        (y > bounds.top() && y <= bounds.bottom())
            .then(|| gpui_kit::point(bounds.left() + height, y))
    }

    /// Opens the shell's menu at `at` (window coordinates; `None` = at the pointer, or the
    /// window's centre). `extended` adds the extended verbs (`Shift`).
    pub(crate) fn open_native_menu(
        &mut self,
        at: Option<Point<Pixels>>,
        extended: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(owner), Some(target)) = (self.hwnd, self.menu_target()) else {
            return;
        };
        tracing::debug!(keyboard = at.is_none(), extended, "shell menu requested");
        let scale = window.scale_factor();
        let point = at.map(|p| {
            // Physical client pixels; window coordinates are logical.
            #[allow(clippy::cast_possible_truncation)]
            let physical = |v: Pixels| (f32::from(v) * scale).round() as i32;
            (physical(p.x), physical(p.y))
        });
        let dark = self
            .features
            .integration_native_menu_dark
            .then(|| self.material().0 == sx_theme::Mode::Dark);
        let options = MenuOptions {
            owner,
            point,
            extended,
            dark,
        };
        // Only the newest menu opens: one asked for meanwhile (a second right-click while the
        // paths were read, or while a long command ran) overtakes this one.
        let ticket = self.menu_ticket.get() + 1;
        self.menu_ticket.set(ticket);
        let (newest, in_job) = (self.menu_ticket.clone(), self.menu_ticket.clone());
        // The paths are read off the UI thread: an offline share must not freeze the window.
        let prepared = crate::ops::off_thread(move || sx_shell::menu::prepare(target));
        cx.spawn_in(window, async move |this, cx| {
            let prepared = match prepared.await {
                Some(Ok(prepared)) if newest.get() == ticket => prepared,
                Some(Err(err)) => {
                    tracing::debug!(error = %err, "shell menu not prepared");
                    let _ = this.update(cx, |this, cx| {
                        this.notice = Some(t!("menu.failed").into());
                        cx.notify();
                    });
                    return;
                }
                _ => return,
            };
            let target = prepared.target().clone();
            let shown = sx_shell::modal::run(move || {
                if in_job.get() != ticket {
                    return Ok(MenuOutcome::Dismissed);
                }
                sx_shell::menu::show(&prepared, options)
            });
            let outcome = shown.await;
            let _ = this.update_in(cx, |this, window, cx| match outcome {
                Ok(Ok(outcome)) => this.on_menu_outcome(outcome, &target, window, cx),
                Ok(Err(err)) => {
                    tracing::debug!(error = %err, "shell menu failed");
                    this.notice = Some(t!("menu.failed").into());
                    cx.notify();
                }
                // The host could not run it, or it panicked (logged there).
                Err(_) => {}
            });
        })
        .detach();
    }

    /// What the chosen command does. Own commands act on what the menu was for, not on what
    /// is selected now.
    fn on_menu_outcome(
        &mut self,
        outcome: MenuOutcome,
        target: &MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (outcome, target) {
            (MenuOutcome::Dismissed, _) => tracing::debug!("shell menu dismissed"),
            (MenuOutcome::Own(OwnVerb::Rename), MenuTarget::Items(paths)) if paths.len() == 1 => {
                self.rename_path(&paths[0], window, cx);
            }
            (MenuOutcome::Own(OwnVerb::Delete), MenuTarget::Items(paths)) => {
                self.delete_paths(paths.clone(), false, cx);
            }
            (MenuOutcome::Own(OwnVerb::Paste(folder)), _) => self.paste_into(folder, cx),
            (MenuOutcome::Own(_), _) => {}
            (MenuOutcome::Invoked { verb }, _) => {
                tracing::debug!(verb, "shell command run");
                // Results do not follow changes by themselves (the watcher has the folder only).
                let changes_nothing = matches!(
                    verb.to_ascii_lowercase().as_str(),
                    "properties" | "open" | "openas" | "edit" | "print" | "copy" | "copyaspath"
                );
                if self.tab().search.is_some() && !changes_nothing {
                    let tab_id = self.tab().id;
                    self.load_tab(tab_id, true, cx);
                }
            }
        }
    }
}
