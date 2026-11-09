//! The themed context menu (FEATURES.md F-CTX-01, F-CTX-03, F-CTX-04; DESIGN.md §8.10). A
//! right-click opens it for the selected items, for the folder's empty area, or for a sidebar
//! item; the Menu key opens it for the selection or the focused sidebar item. Every entry is a
//! command of the `CommandRegistry` where one exists (its label and shortcut, as in the
//! palette), and "Show more options" opens Windows' own menu for the same items (F-CTX-02).
//!
//! `Up`/`Down` choose, `Right`/`Left` enter and leave a submenu, `Enter`/`Space` or a click runs,
//! `Esc` or a click outside closes (a click outside only closes, as in Windows). While the menu is
//! open the window's shortcuts are off (key context `ContextMenu`) and focus stays in it; it goes
//! back where it was afterwards, and list commands run on the list.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, Animation, AnimationExt as _, AnyElement, ClickEvent, Context, ElementId, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Pixels,
    Point, SharedString, StatefulInteractiveElement, Styled as _, Window, div, px,
};
use rust_i18n::t;
use sx_core::{CommandContext, Location};
use sx_fs::Column;
use sx_shell::menu::MenuTarget;
use sx_theme::Token;

use crate::actions;
use crate::theme::{ActiveTokens as _, HAIRLINE};
use crate::workspace::Workspace;

/// What an entry does.
enum Run {
    /// A registry command, run on the list or the window.
    Action(Box<dyn Action>, CommandContext),
    /// Windows' own menu for these items, where this menu was.
    Native(MenuTarget),
    Properties(MenuTarget),
    OpenTab(Location),
    Go(Location),
    SortBy(Column),
    PasteInto(PathBuf),
    Pin(PathBuf),
    Unpin(PathBuf),
    MovePin(PathBuf, isize),
    /// Dropped items, put where they were dropped (the menu of an `Ask` drop).
    Drop(Vec<PathBuf>, PathBuf, sx_ops::Mode),
    /// Closes the menu and does nothing else.
    Close,
    Submenu(Vec<Entry>),
}

struct Entry {
    label: SharedString,
    keys: SharedString,
    enabled: bool,
    /// One of a set of choices (the sort order); `checked` marks the current one.
    radio: bool,
    checked: bool,
    /// `None` draws a separator.
    run: Option<Run>,
}

impl Entry {
    fn new(label: impl Into<SharedString>, enabled: bool, run: Run) -> Self {
        Self {
            label: label.into(),
            keys: SharedString::default(),
            enabled,
            radio: false,
            checked: false,
            run: Some(run),
        }
    }

    /// A registry command: its label and first shortcut.
    fn command(id: &'static str, enabled: bool, run: Run) -> Self {
        let keys = sx_core::command::BUILTIN
            .iter()
            .find(|spec| spec.id == id)
            .and_then(|spec| spec.bindings.first())
            .map(|keys| crate::hotswitch::keys_text(keys))
            .unwrap_or_default();
        Self {
            keys: keys.into(),
            ..Self::new(t!(format!("command.{id}")), enabled, run)
        }
    }

    fn action(id: &'static str, enabled: bool, action: Box<dyn Action>) -> Self {
        let context = sx_core::command::BUILTIN
            .iter()
            .find(|spec| spec.id == id)
            .map_or(CommandContext::Workspace, |spec| spec.context);
        Self::command(id, enabled, Run::Action(action, context))
    }

    fn separator() -> Self {
        Self {
            label: SharedString::default(),
            keys: SharedString::default(),
            enabled: false,
            radio: false,
            checked: false,
            run: None,
        }
    }

    fn is_choice(&self) -> bool {
        self.run.is_some() && self.enabled
    }

    fn submenu(&self) -> Option<&[Entry]> {
        match &self.run {
            Some(Run::Submenu(entries)) => Some(entries),
            _ => None,
        }
    }
}

pub(crate) struct ContextMenu {
    entries: Vec<Entry>,
    /// Where it was asked for, in window coordinates.
    at: Point<Pixels>,
    selected: Option<usize>,
    /// The entry whose submenu is shown, and the row chosen in it (the keyboard is in the
    /// submenu only while that row is set).
    open_sub: Option<usize>,
    sub_selected: Option<usize>,
    /// The last move was from the keyboard: the chosen row shows the focus ring.
    keyboard: bool,
    focus: FocusHandle,
    /// Where focus was before the menu opened.
    restore: Option<FocusHandle>,
    /// Which opening this is, so the panel fades in once.
    opened: u64,
    /// The window has been active while the menu was open. A menu closes when the window loses
    /// activation; one opened by a drop from another app may start in an inactive window.
    seen_active: bool,
}

impl ContextMenu {
    /// Notes this frame's activation; `false` when the menu should close now.
    pub(crate) fn keep_open(&mut self, active: bool) -> bool {
        self.seen_active |= active;
        active || !self.seen_active
    }
}

impl std::fmt::Debug for ContextMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextMenu")
            .field("entries", &self.entries.len())
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

/// The next row that can be chosen, from `from` in direction `step`, wrapping around.
fn next_choice(entries: &[Entry], from: Option<usize>, step: isize) -> Option<usize> {
    let count = entries.len() as isize;
    if count == 0 {
        return None;
    }
    let mut ix = match from {
        Some(ix) => ix as isize,
        None if step > 0 => -1,
        None => count,
    };
    for _ in 0..count {
        ix = (ix + step).rem_euclid(count);
        if entries[ix as usize].is_choice() {
            return Some(ix as usize);
        }
    }
    None
}

enum KeyResult {
    Redraw,
    Close,
    /// Run the entry (and the submenu row, if any).
    Run(usize, Option<usize>),
}

impl ContextMenu {
    pub(crate) fn focus_menu(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        window.focus(&self.focus, cx);
    }

    fn in_submenu(&self) -> Option<&[Entry]> {
        self.sub_selected?;
        self.entries.get(self.open_sub?)?.submenu()
    }

    fn enter_submenu(&mut self) -> bool {
        let Some(ix) = self.selected else {
            return false;
        };
        let Some(sub) = self.entries.get(ix).and_then(Entry::submenu) else {
            return false;
        };
        self.open_sub = Some(ix);
        self.sub_selected = next_choice(sub, None, 1);
        true
    }

    fn key_down(&mut self, key: &str) -> Option<KeyResult> {
        self.keyboard = true;
        let step = match key {
            "down" => 1,
            "up" => -1,
            _ => 0,
        };
        if let Some(sub) = self.in_submenu() {
            match key {
                "down" | "up" => self.sub_selected = next_choice(sub, self.sub_selected, step),
                "left" | "escape" => self.sub_selected = None,
                "enter" | "space" => {
                    return Some(KeyResult::Run(self.open_sub?, self.sub_selected));
                }
                _ => return None,
            }
            return Some(KeyResult::Redraw);
        }
        match key {
            "down" | "up" => {
                self.selected = next_choice(&self.entries, self.selected, step);
                // A submenu shown by the pointer goes when the keyboard moves away.
                if self.open_sub != self.selected {
                    self.open_sub = None;
                }
            }
            "escape" => return Some(KeyResult::Close),
            "right" => {
                self.enter_submenu();
            }
            "enter" | "space" => {
                if !self.enter_submenu() {
                    return Some(KeyResult::Run(self.selected?, None));
                }
            }
            _ => return None,
        }
        Some(KeyResult::Redraw)
    }
}

impl Workspace {
    /// Where dropped items go, asked at the drop (`dnd.default_action = ask`, or a right-button
    /// drag; DEVELOPER-MODE.md §5.4).
    pub(crate) fn open_drop_menu(
        &mut self,
        paths: Vec<PathBuf>,
        dest: PathBuf,
        at: Point<Pixels>,
        allowed: sx_shell::dnd::Allowed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A drop from another app leaves that app in front: bring this window forward, or the
        // menu would sit behind it.
        window.activate_window();
        // Moving items to where they already are does nothing.
        let here = paths.iter().all(|path| {
            path.parent()
                .is_some_and(|parent| crate::workspace::same_path(parent, &dest))
        });
        // Only what the source allows: a copy-only source is never moved.
        let entries = vec![
            Entry::new(
                t!("menu.drop_copy"),
                allowed.copy,
                Run::Drop(paths.clone(), dest.clone(), sx_ops::Mode::Copy),
            ),
            Entry::new(
                t!("menu.drop_move"),
                allowed.moves && !here,
                Run::Drop(paths, dest, sx_ops::Mode::Move),
            ),
            Entry::separator(),
            Entry::new(t!("menu.drop_cancel"), true, Run::Close),
        ];
        self.show_menu(entries, at, false, window, cx);
    }

    /// A right-click in the list (or the Menu key): the menu of the selection, or of the
    /// folder's empty area with nothing selected.
    pub(crate) fn open_list_menu(
        &mut self,
        at: Point<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = if self.tab().selection.is_empty() {
            self.background_entries()
        } else {
            self.item_entries()
        };
        self.show_menu(entries, at, keyboard, window, cx);
    }

    /// A right-click on a sidebar item (or the Menu key on it).
    pub(crate) fn open_sidebar_menu(
        &mut self,
        location: &Location,
        at: Point<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.sidebar_entries(location);
        self.show_menu(entries, at, keyboard, window, cx);
    }

    fn show_menu(
        &mut self,
        entries: Vec<Entry>,
        at: Point<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() || self.ops.dialog.is_some() {
            return;
        }
        self.palette = None;
        let restore = match self.context_menu.take() {
            Some(open) => open.restore,
            None => window.focused(cx),
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.menu_openings += 1;
        // From the keyboard the first row is chosen at once, so the focus shows.
        let selected = keyboard.then(|| next_choice(&entries, None, 1)).flatten();
        self.context_menu = Some(ContextMenu {
            entries,
            at,
            selected,
            open_sub: None,
            sub_selected: None,
            keyboard,
            focus,
            restore,
            opened: self.menu_openings,
            seen_active: window.is_window_active(),
        });
        cx.notify();
    }

    /// The menu of the selected items (F-CTX-01).
    fn item_entries(&self) -> Vec<Entry> {
        let tab = self.tab();
        let writable = !self.features.safety_readonly_mode;
        let rows: Vec<usize> = tab.selection.iter().map(|r| r as usize).collect();
        let folder_row = |row: usize| {
            tab.snapshot()
                .and_then(|s| s.row(row))
                .is_some_and(|e| e.kind.is_container())
        };
        let all_folders = !rows.is_empty() && rows.iter().all(|&r| folder_row(r));
        let paths = self.selected_paths();
        let single_folder = (rows.len() == 1 && all_folders)
            .then(|| paths.first().cloned())
            .flatten();
        let target = self.menu_target();
        let mut entries = vec![Entry::action(
            "file.open",
            true,
            Box::new(actions::OpenSelected),
        )];
        if all_folders {
            entries.push(Entry::action(
                "file.open_new_tab",
                true,
                Box::new(actions::OpenInNewTab),
            ));
        }
        entries.push(Entry::separator());
        entries.push(Entry::action(
            "file.cut",
            writable,
            Box::new(actions::FileCut),
        ));
        entries.push(Entry::action(
            "file.copy",
            true,
            Box::new(actions::FileCopy),
        ));
        if let Some(folder) = &single_folder {
            // Into that folder: not the Ctrl+V of the folder shown, so no shortcut.
            entries.push(Entry::new(
                t!("menu.paste_into"),
                writable && sx_shell::clipboard::has_files(),
                Run::PasteInto(folder.clone()),
            ));
        }
        entries.push(Entry::separator());
        entries.push(Entry::action(
            "file.rename",
            writable && rows.len() == 1,
            Box::new(actions::FileRename),
        ));
        entries.push(Entry::action(
            "file.delete",
            writable,
            Box::new(actions::FileDelete),
        ));
        entries.push(Entry::separator());
        entries.push(Entry::action(
            "file.copy_path",
            true,
            Box::new(actions::FileCopyPath),
        ));
        if let Some(folder) = single_folder {
            let locked = self.features.sidebar_pins_locked;
            entries.push(if self.is_pinned(&folder) {
                Entry::new(t!("nav.unpin"), !locked, Run::Unpin(folder))
            } else {
                Entry::new(t!("nav.pin"), !locked, Run::Pin(folder))
            });
        }
        if let Some(target) = target {
            entries.push(Entry::separator());
            entries.push(Entry::command(
                "file.properties",
                true,
                Run::Properties(target.clone()),
            ));
            entries.push(Entry::command(
                "menu.more_options",
                true,
                Run::Native(target),
            ));
        }
        entries
    }

    /// The menu of the folder's empty area (F-CTX-03).
    fn background_entries(&self) -> Vec<Entry> {
        let Some(dir) = self.tab().location().as_path().map(PathBuf::from) else {
            return Vec::new();
        };
        let writable = !self.features.safety_readonly_mode;
        let sort = self.tab().sort.column;
        let sorts = [
            (Column::Name, t!("details.name")),
            (Column::Modified, t!("details.modified")),
            (Column::Type, t!("details.type")),
            (Column::Size, t!("details.size")),
        ]
        .into_iter()
        .map(|(column, label)| Entry {
            radio: true,
            checked: sort == column,
            ..Entry::new(label, true, Run::SortBy(column))
        })
        .collect();
        let pinned = self.is_pinned(&dir);
        vec![
            Entry::new(t!("menu.sort_by"), true, Run::Submenu(sorts)),
            Entry::action("nav.refresh", true, Box::new(actions::NavRefresh)),
            Entry::separator(),
            Entry::action(
                "file.paste",
                writable && sx_shell::clipboard::has_files(),
                Box::new(actions::FilePaste),
            ),
            Entry::action(
                "file.new_folder",
                writable,
                Box::new(actions::FileNewFolder),
            ),
            Entry::separator(),
            Entry::new(
                if pinned {
                    t!("nav.unpin")
                } else {
                    t!("nav.pin")
                },
                !self.features.sidebar_pins_locked,
                if pinned {
                    Run::Unpin(dir.clone())
                } else {
                    Run::Pin(dir.clone())
                },
            ),
            Entry::separator(),
            Entry::command(
                "file.properties",
                true,
                Run::Properties(MenuTarget::Background(dir.clone())),
            ),
            Entry::command(
                "menu.more_options",
                true,
                Run::Native(MenuTarget::Background(dir)),
            ),
        ]
    }

    /// The menu of a sidebar item (F-CTX-04). Its entries have no shortcuts: those act on the
    /// list, not on the sidebar item.
    fn sidebar_entries(&self, location: &Location) -> Vec<Entry> {
        let mut entries = vec![
            Entry::new(t!("command.file.open"), true, Run::Go(location.clone())),
            Entry::new(
                t!("command.file.open_new_tab"),
                true,
                Run::OpenTab(location.clone()),
            ),
        ];
        let Some(path) = location.as_path().map(PathBuf::from) else {
            return entries;
        };
        if let Some(ix) = self
            .pins
            .iter()
            .position(|pin| crate::workspace::same_path(&pin.path, &path))
        {
            let editable = !self.features.sidebar_pins_locked;
            entries.push(Entry::separator());
            entries.push(Entry::new(
                t!("nav.unpin"),
                editable,
                Run::Unpin(path.clone()),
            ));
            entries.push(Entry::new(
                t!("menu.move_up"),
                editable && ix > 0,
                Run::MovePin(path.clone(), -1),
            ));
            entries.push(Entry::new(
                t!("menu.move_down"),
                editable && ix + 1 < self.pins.len(),
                Run::MovePin(path.clone(), 1),
            ));
        }
        let target = MenuTarget::Items(vec![path]);
        entries.push(Entry::separator());
        entries.push(Entry::new(
            t!("command.file.properties"),
            true,
            Run::Properties(target.clone()),
        ));
        entries.push(Entry::new(
            t!("command.menu.more_options"),
            true,
            Run::Native(target),
        ));
        entries
    }

    /// Closes the menu; focus goes back where it was.
    pub(crate) fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        match menu.restore {
            Some(handle) => window.focus(&handle, cx),
            None => window.focus(&self.list_focus, cx),
        }
        cx.notify();
    }

    fn menu_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.context_menu else {
            return;
        };
        let Some(result) = menu.key_down(event.keystroke.key.as_str()) else {
            return;
        };
        cx.stop_propagation();
        match result {
            KeyResult::Redraw => cx.notify(),
            KeyResult::Close => self.close_context_menu(window, cx),
            KeyResult::Run(ix, sub) => self.run_menu_entry(ix, sub, window, cx),
        }
    }

    /// Runs an entry (or a row of its submenu). A disabled entry does nothing.
    fn run_menu_entry(
        &mut self,
        ix: usize,
        sub: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let choosable = self.context_menu.as_ref().is_some_and(|menu| {
            let entry = menu.entries.get(ix);
            match (entry, sub) {
                (Some(entry), Some(row)) => entry
                    .submenu()
                    .and_then(|s| s.get(row))
                    .is_some_and(Entry::is_choice),
                (Some(entry), None) => entry.is_choice() && entry.submenu().is_none(),
                (None, _) => false,
            }
        });
        if !choosable {
            return;
        }
        let Some(mut menu) = self.context_menu.take() else {
            return;
        };
        let at = menu.at;
        let mut entry = menu.entries.swap_remove(ix);
        let run = match (sub, entry.run.take()) {
            (Some(row), Some(Run::Submenu(mut rows))) => rows.swap_remove(row).run,
            (_, run) => run,
        };
        let Some(run) = run else {
            return;
        };
        // List commands act on the list; the rest where focus was.
        let list = matches!(run, Run::Action(_, CommandContext::List));
        match &menu.restore {
            Some(handle) if !list => window.focus(handle, cx),
            _ => window.focus(&self.list_focus, cx),
        }
        match run {
            Run::Action(action, _) => window.defer(cx, move |window, cx| {
                window.dispatch_action(action, cx);
            }),
            Run::Native(target) => self.open_native_menu_for(target, Some(at), false, window, cx),
            Run::Properties(target) => self.show_properties(target, cx),
            Run::OpenTab(location) => {
                let active = self.active;
                self.open_tab(location, cx);
                self.active = active;
            }
            Run::Go(location) => self.navigate(location, cx),
            Run::SortBy(column) => {
                if self.tab().sort.column != column {
                    self.sort_by(column, cx);
                }
            }
            Run::PasteInto(folder) => self.paste_into(folder, cx),
            // Choosing here is the confirmation: no second question about moving folders.
            Run::Drop(paths, dest, mode) => {
                let quiet = !self.features.dnd_undo_toast;
                self.transfer(paths, dest, mode, quiet, cx);
            }
            Run::Pin(path) => self.edit_pins(|pins| pins.push(path), cx),
            Run::Unpin(path) => self.edit_pins(
                |pins| pins.retain(|p| !crate::workspace::same_path(p, &path)),
                cx,
            ),
            Run::MovePin(path, step) => self.edit_pins(
                |pins| {
                    if let Some(from) = pins
                        .iter()
                        .position(|p| crate::workspace::same_path(p, &path))
                    {
                        let to = from.saturating_add_signed(step).min(pins.len() - 1);
                        pins.swap(from, to);
                    }
                },
                cx,
            ),
            Run::Close | Run::Submenu(_) => {}
        }
        cx.notify();
    }

    /// The pointer entered (or left) a row: it becomes the chosen one. Leaving keeps a
    /// submenu's parent chosen while the pointer goes into the submenu.
    fn hover_menu_row(
        &mut self,
        ix: usize,
        sub: Option<usize>,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &mut self.context_menu else {
            return;
        };
        if hovered {
            menu.keyboard = false;
            if let Some(row) = sub {
                menu.sub_selected = Some(row);
            } else {
                menu.selected = Some(ix);
                let has_sub = menu.entries.get(ix).is_some_and(|e| e.submenu().is_some());
                menu.open_sub = has_sub.then_some(ix);
                menu.sub_selected = None;
            }
        } else if !menu.keyboard {
            match sub {
                Some(row) if menu.sub_selected == Some(row) => menu.sub_selected = None,
                None if menu.selected == Some(ix) && menu.open_sub != Some(ix) => {
                    menu.selected = None;
                }
                _ => {}
            }
        }
        cx.notify();
    }

    pub(crate) fn render_context_menu(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let t = cx.tokens();
        let width = t.px(Token::SizeMenuWidth);
        let viewport = window.viewport_size();
        let margin = t.px(Token::SpaceSm);
        let overlap = t.px(Token::SpaceXs);
        let height = menu_height(&menu.entries, cx);
        // A menu with a submenu keeps room for it on the right, so neither covers the other.
        let has_sub = menu.entries.iter().any(|e| e.submenu().is_some());
        let room = if has_sub {
            width * 2.0 - overlap
        } else {
            width
        };
        let x = menu.at.x.min(viewport.width - room - margin).max(margin);
        // Above the pointer when it does not fit below.
        let y = if menu.at.y + height > viewport.height - margin {
            (menu.at.y - height).max(margin)
        } else {
            menu.at.y
        };
        let main = self.menu_panel(&menu.entries, None, menu, cx);
        let sub = menu.open_sub.and_then(|ix| {
            let entries = menu.entries.get(ix)?.submenu()?;
            let sub_height = menu_height(entries, cx);
            let right = x + width - overlap;
            let sx = if right + width <= viewport.width - margin {
                right
            } else {
                (x - width + overlap).max(margin)
            };
            // The submenu's first row level with its entry.
            let sy = (y + rows_height(&menu.entries[..ix], cx))
                .min(viewport.height - sub_height - margin)
                .max(margin);
            Some((self.menu_panel(entries, Some(ix), menu, cx), sx, sy))
        });
        let ms = t.number(Token::MotionBase).max(0.0);
        let animate = self.features.perf_animations != "off" && ms > 0.0;
        let fade = |panel: gpui_kit::Stateful<gpui_kit::Div>, key: &'static str| -> AnyElement {
            if animate {
                panel
                    .with_animation(
                        ElementId::NamedInteger(key.into(), menu.opened),
                        Animation::new(Duration::from_millis(ms as u64))
                            .with_easing(gpui_kit::ease_out_quint()),
                        gpui_kit::Styled::opacity,
                    )
                    .into_any_element()
            } else {
                panel.into_any_element()
            }
        };
        Some(
            div()
                .id("menu-scrim")
                .absolute()
                .inset_0()
                .occlude()
                .track_focus(&menu.focus)
                .key_context("ContextMenu")
                .capture_key_down(cx.listener(Self::menu_key_down))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_context_menu(window, cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, window, cx| {
                        // A click outside only closes the menu: its release must not open
                        // another one for the old selection.
                        this.swallow_right_up = true;
                        this.close_context_menu(window, cx);
                    }),
                )
                .child(fade(main.absolute().left(x).top(y), "menu-in"))
                .children(
                    sub.map(|(panel, sx, sy)| {
                        fade(panel.absolute().left(sx).top(sy), "submenu-in")
                    }),
                )
                .into_any_element(),
        )
    }

    /// One menu panel: the main menu, or the submenu of entry `parent`.
    fn menu_panel(
        &self,
        entries: &[Entry],
        parent: Option<usize>,
        menu: &ContextMenu,
        cx: &Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let t = cx.tokens();
        let selected = match parent {
            Some(_) => menu.sub_selected,
            None => menu.selected,
        };
        let count = entries.iter().filter(|e| e.run.is_some()).count();
        // Where the keyboard is: in the submenu once it has a row, else here.
        let here = parent.is_some() || menu.sub_selected.is_none();
        let mut position = 0;
        let label: SharedString = match parent {
            Some(ix) => menu
                .entries
                .get(ix)
                .map(|e| e.label.clone())
                .unwrap_or_default(),
            None => t!("menu.label").into(),
        };
        div()
            .id(match parent {
                Some(ix) => ElementId::NamedInteger("submenu".into(), ix as u64),
                None => ElementId::Name("context-menu".into()),
            })
            .role(gpui_kit::Role::Menu)
            .aria_label(label)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .w(t.px(Token::SizeMenuWidth))
            .p(t.px(Token::SpaceXs))
            .rounded(t.px(Token::RadiusMd))
            .bg(t.paint(Token::SurfaceOverlay))
            .border_1()
            .border_color(t.color(Token::BorderSubtle))
            .shadow(t.shadow(Token::ShadowPopover))
            .children(entries.iter().enumerate().map(|(row, entry)| {
                if entry.run.is_none() {
                    return div()
                        .my(t.px(Token::SpaceXs))
                        .mx(t.px(Token::SpaceSm))
                        .border_b_1()
                        .border_color(t.color(Token::Divider))
                        .into_any_element();
                }
                position += 1;
                let (ix, sub) = match parent {
                    Some(ix) => (ix, Some(row)),
                    None => (row, None),
                };
                let open = parent.is_none() && menu.open_sub == Some(row);
                let chosen = selected == Some(row) || open;
                let look = RowLook {
                    chosen,
                    active: here && selected == Some(row),
                    ring: chosen && here && menu.keyboard,
                    open,
                    position,
                    count,
                };
                self.menu_row(entry, ix, sub, look, cx)
            }))
    }

    fn menu_row(
        &self,
        entry: &Entry,
        ix: usize,
        sub: Option<usize>,
        look: RowLook,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.tokens();
        let (hover, pressed) = (t.color(Token::HoverBg), t.color(Token::PressedBg));
        let icon = t.px(Token::SizeIconSm);
        let radius = t.px(Token::RadiusSm);
        let tint = |token: Token| {
            if entry.enabled {
                t.color(token)
            } else {
                t.color(Token::FgDisabled)
            }
        };
        let row_id = ElementId::NamedInteger(
            "menu-row".into(),
            ((ix as u64) << 16) | sub.map_or(0xFFFF, |r| r as u64),
        );
        let has_sub = entry.submenu().is_some();
        div()
            .id(row_id)
            .role(if entry.radio {
                gpui_kit::Role::MenuItemRadio
            } else {
                gpui_kit::Role::MenuItem
            })
            .aria_label(entry.label.clone())
            .aria_position_in_set(look.position)
            .aria_size_of_set(look.count)
            .when(entry.radio, |el| {
                el.aria_toggled(if entry.checked {
                    gpui_kit::Toggled::True
                } else {
                    gpui_kit::Toggled::False
                })
            })
            .when(has_sub, |el| el.aria_expanded(look.open))
            .when(!entry.keys.is_empty(), |el| {
                el.aria_keyshortcuts(entry.keys.clone())
            })
            // Read out as focused while the keyboard is on it (focus stays on the menu).
            .when(
                look.active,
                StatefulInteractiveElement::aria_active_descendant,
            )
            .relative()
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceSm))
            .h(t.px(Token::SizeControl))
            .px(t.px(Token::SpaceSm))
            .rounded(radius)
            .text_color(tint(Token::FgPrimary))
            // One row is lit, the chosen one (by pointer or keyboard), never two.
            .when(look.chosen && entry.enabled, |el| el.bg(hover))
            .when(entry.enabled, |el| el.active(move |s| s.bg(pressed)))
            .when(look.ring, |el| el.child(crate::focus::ring(radius, cx)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.hover_menu_row(ix, sub, *hovered, cx);
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.run_menu_entry(ix, sub, window, cx);
            }))
            .child(div().flex_none().w(icon).when(entry.checked, |el| {
                el.child(Icon::new(IconName::Check).size(icon))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(entry.label.clone()),
            )
            .when(!entry.keys.is_empty(), |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_size(t.px(Token::FontSizeCaption))
                        .text_color(tint(Token::FgSecondary))
                        .child(entry.keys.clone()),
                )
            })
            .when(has_sub, |el| {
                el.child(Icon::new(IconName::ChevronRight).size(icon * 0.75))
            })
            .into_any_element()
    }
}

/// How a row is drawn. (Independent facts of one row, not a state machine.)
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct RowLook {
    /// Lit: under the pointer, chosen by the keyboard, or the parent of the shown submenu.
    chosen: bool,
    /// The keyboard's row (read out as focused).
    active: bool,
    ring: bool,
    /// Its submenu is shown.
    open: bool,
    position: usize,
    count: usize,
}

/// The height of these rows and separators, without the panel's padding and border.
fn rows_height(entries: &[Entry], cx: &gpui_kit::App) -> Pixels {
    let t = cx.tokens();
    let row = t.px(Token::SizeControl);
    let separator = t.px(Token::SpaceXs) * 2.0 + px(HAIRLINE);
    entries
        .iter()
        .map(|e| if e.run.is_some() { row } else { separator })
        .fold(px(0.0), |sum, h| sum + h)
}

/// The height of a menu panel with these entries.
fn menu_height(entries: &[Entry], cx: &gpui_kit::App) -> Pixels {
    rows_height(entries, cx) + cx.tokens().px(Token::SpaceXs) * 2.0 + px(HAIRLINE * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(enabled: bool) -> Entry {
        Entry::new("x", enabled, Run::SortBy(Column::Name))
    }

    #[test]
    fn keyboard_skips_separators_and_disabled_rows() {
        let entries = vec![entry(true), Entry::separator(), entry(false), entry(true)];
        assert_eq!(next_choice(&entries, None, 1), Some(0));
        assert_eq!(next_choice(&entries, Some(0), 1), Some(3));
        assert_eq!(next_choice(&entries, Some(3), 1), Some(0));
        assert_eq!(next_choice(&entries, None, -1), Some(3));
        assert_eq!(next_choice(&[Entry::separator()], None, 1), None);
    }
}
