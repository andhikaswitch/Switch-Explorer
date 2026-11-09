//! The command palette (FEATURES.md F-CMD-01, DESIGN.md §8.9). `Ctrl+Shift+P` / `F1` opens a box
//! near the top of the window that finds commands, Hot Switch switches and places by fuzzy
//! matching (ADR-023); `Ctrl+P` opens it for folders only (quick open, F-NAV-07): open tabs,
//! pinned folders, folders opened this session, drives. `Up`/`Down` choose, `Enter` or a click runs, `Esc` or a click outside
//! closes. Running an item closes the palette; list commands run on the list, others where focus
//! was. A switch runs like its Hot Switch key, OSD included, even with the Hot Switch keys off.
//! Commands that cannot run now (Forward without history, Rename without a selection) are shown
//! disabled.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, AppContext as _, ClickEvent, Context, ElementId, Entity, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    ParentElement as _, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, UniformListScrollHandle, Window, div, uniform_list,
};
use rust_i18n::t;
use sx_core::{CommandContext, Location};
use sx_settings::{DEFAULT_BINDINGS, HotSwitchAction};
use sx_theme::Token;

use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Rows shown before the list scrolls.
const VISIBLE_ROWS: usize = 10;

/// The Hot Switch panel: rows it shows when the window is short, and at most.
const MIN_PANEL_ROWS: usize = 4;
const MAX_PANEL_ROWS: usize = 20;

/// Folders remembered for quick open, most recent first (this session only, in memory).
pub(crate) const RECENT_FOLDERS: usize = 50;

/// What the palette offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteMode {
    /// Commands, switches and places (`Ctrl+Shift+P`).
    Commands,
    /// Folders to go to (`Ctrl+P`).
    Folders,
    /// The Hot Switch panel (`Ctrl+K, K`, DESIGN.md §8.4): every switch with its state; running
    /// one keeps the panel open.
    Switches,
}

/// What an item does.
enum Run {
    Command(Box<dyn Action>, CommandContext),
    /// A Hot Switch binding, by index in `DEFAULT_BINDINGS`.
    Switch(usize),
    Go(Location),
    /// An open tab, by id: it is brought forward.
    Tab(u64),
    /// A section title of the Hot Switch panel: never chosen, hidden while searching.
    Header,
}

struct Item {
    label: SharedString,
    /// A muted word after the label: the place's path. (A switch's state is read when drawn.)
    detail: SharedString,
    /// The shortcut, as Windows writes it.
    keys: SharedString,
    run: Run,
    /// Whether it can run now.
    enabled: bool,
}

/// An item that matches what is typed, with the matched characters (byte offsets) of its label.
struct Shown {
    item: usize,
    positions: Vec<usize>,
    /// Its place among the rows that can be chosen, from 1 (0 for a section title): what a
    /// screen reader says ("2 of 11").
    place: usize,
}

pub(crate) struct Palette {
    mode: PaletteMode,
    input: Entity<InputState>,
    items: Vec<Item>,
    shown: Vec<Shown>,
    selected: usize,
    /// How many shown rows can be chosen.
    choices: usize,
    scroll: UniformListScrollHandle,
    /// Where focus was before the palette opened.
    restore: Option<FocusHandle>,
    _subscription: Subscription,
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette")
            .field("items", &self.items.len())
            .field("shown", &self.shown.len())
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl Palette {
    /// The items matching `query`: label matches first (best first), then places matched by
    /// their path. All items, in order, for an empty query.
    fn filter(&mut self, query: &str) {
        let mut scored: Vec<(bool, i32, Shown)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(ix, item)| {
                if matches!(item.run, Run::Header) {
                    let shown = Shown {
                        item: ix,
                        positions: Vec::new(),
                        place: 0,
                    };
                    return query.trim().is_empty().then_some((true, 0, shown));
                }
                if let Some(found) = sx_search::fuzzy_match(query, &item.label) {
                    let shown = Shown {
                        item: ix,
                        positions: found.positions,
                        place: 0,
                    };
                    return Some((true, found.score, shown));
                }
                if !matches!(item.run, Run::Go(_) | Run::Tab(_)) {
                    return None;
                }
                let found = sx_search::fuzzy_match(query, &item.detail)?;
                let shown = Shown {
                    item: ix,
                    positions: Vec::new(),
                    place: 0,
                };
                Some((false, found.score, shown))
            })
            .collect();
        if !query.trim().is_empty() {
            // Stable: equal keys keep the order of the list.
            scored.sort_by_key(|(label, score, _)| std::cmp::Reverse((*label, *score)));
        }
        self.shown = scored.into_iter().map(|(_, _, shown)| shown).collect();
        let items = &self.items;
        let mut place = 0;
        for shown in &mut self.shown {
            if items
                .get(shown.item)
                .is_some_and(|item| !matches!(item.run, Run::Header))
            {
                place += 1;
                shown.place = place;
            }
        }
        self.choices = place;
        self.selected = (0..self.shown.len())
            .find(|&row| self.choosable(row))
            .unwrap_or(0);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Whether a shown row can be chosen (section titles cannot).
    fn choosable(&self, row: usize) -> bool {
        self.shown
            .get(row)
            .and_then(|shown| self.items.get(shown.item))
            .is_some_and(|item| !matches!(item.run, Run::Header))
    }

    /// One row up or down wraps around; a page stops at the ends.
    fn step(&mut self, delta: isize, wrap: bool) {
        let count = self.shown.len() as isize;
        if count == 0 {
            return;
        }
        let place = |row: isize| {
            if wrap {
                row.rem_euclid(count)
            } else {
                row.clamp(0, count - 1)
            }
        };
        let mut next = place(self.selected as isize + delta);
        // Past section titles, in the direction of the move.
        let way = delta.signum();
        for _ in 0..count {
            if self.choosable(next as usize) || way == 0 {
                break;
            }
            let further = place(next + way);
            if further == next {
                break;
            }
            next = further;
        }
        if !self.choosable(next as usize) {
            // A page ran into a title at an end: the nearest row back from it.
            let back = if way > 0 {
                (0..next).rev().find(|&row| self.choosable(row as usize))
            } else {
                (next + 1..count).find(|&row| self.choosable(row as usize))
            };
            let Some(back) = back else {
                return;
            };
            next = back;
        }
        self.selected = next as usize;
        // Going up to the first row of a section shows its title too.
        let above = self.selected.checked_sub(1);
        let target = match above {
            Some(title) if way < 0 && !self.choosable(title) => title,
            _ => self.selected,
        };
        self.scroll.scroll_to_item(target, ScrollStrategy::Nearest);
    }

    pub(crate) fn focus_input(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        let handle = self.input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }
}

impl Workspace {
    /// `Ctrl+Shift+P` / `F1` (commands) or `Ctrl+P` (folders). Asking for the other mode while
    /// the palette is open switches to it.
    pub(crate) fn open_palette(
        &mut self,
        mode: PaletteMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ops.dialog.is_some() {
            return;
        }
        self.context_menu = None;
        let restore = match self.palette.take() {
            Some(open) if open.mode == mode => {
                self.palette = Some(open);
                return;
            }
            Some(open) => open.restore,
            None => window.focused(cx),
        };
        let placeholder = match mode {
            PaletteMode::Commands => t!("palette.placeholder"),
            PaletteMode::Folders => t!("palette.placeholder_folders"),
            PaletteMode::Switches => t!("palette.placeholder_switches"),
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        let subscription = cx.subscribe_in(&input, window, Self::on_palette_event);
        input.update(cx, |input, cx| input.focus(window, cx));
        let items = match mode {
            PaletteMode::Commands => self.palette_items(),
            PaletteMode::Folders => self.folder_items(),
            PaletteMode::Switches => self.switch_items(),
        };
        let mut palette = Palette {
            mode,
            input,
            items,
            shown: Vec::new(),
            selected: 0,
            choices: 0,
            scroll: UniformListScrollHandle::new(),
            restore,
            _subscription: subscription,
        };
        palette.filter("");
        self.palette = Some(palette);
        cx.notify();
    }

    /// The Hot Switch panel, with `binding` chosen when given (a hint's switch).
    pub(crate) fn open_switch_panel(
        &mut self,
        binding: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(PaletteMode::Switches, window, cx);
        if let (Some(binding), Some(palette)) = (binding, self.palette.as_mut())
            && let Some(row) = palette.shown.iter().position(|shown| {
                palette
                    .items
                    .get(shown.item)
                    .is_some_and(|item| matches!(item.run, Run::Switch(b) if b == binding))
            })
        {
            palette.selected = row;
            palette.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    /// Quick open: the open tabs' folders, pinned folders, folders opened this session, then the
    /// drives; each folder once, where it first appears.
    fn folder_items(&self) -> Vec<Item> {
        // Each folder once: Windows paths ignore case.
        let mut seen = std::collections::HashSet::new();
        let mut first = |path: &std::path::Path| seen.insert(path.to_string_lossy().to_lowercase());
        let mut items = Vec::new();
        let name = |path: &std::path::Path| -> SharedString {
            path.file_name()
                .map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
                .into()
        };
        for tab in &self.tabs {
            if let Some(path) = tab.location().as_path()
                && first(path)
            {
                items.push(Item {
                    label: name(path),
                    detail: path.display().to_string().into(),
                    keys: t!("palette.open_tab").into(),
                    run: Run::Tab(tab.id),
                    enabled: true,
                });
            }
        }
        let folder = |label: SharedString, path: &std::path::Path| Item {
            label,
            detail: path.display().to_string().into(),
            keys: SharedString::default(),
            run: Run::Go(Location::Path(path.to_path_buf())),
            enabled: true,
        };
        for pin in &self.pins {
            if first(&pin.path) {
                items.push(folder(pin.name.clone(), &pin.path));
            }
        }
        for path in &self.recent_folders {
            if first(path) {
                items.push(folder(name(path), path));
            }
        }
        for drive in &self.drives {
            if first(&drive.root) {
                items.push(folder(crate::format::drive_label(drive), &drive.root));
            }
        }
        items
    }

    /// The Hot Switch panel: drag switches first, then the view and safety ones, under titles.
    fn switch_items(&self) -> Vec<Item> {
        let header = |label: SharedString| Item {
            label,
            detail: SharedString::default(),
            keys: SharedString::default(),
            run: Run::Header,
            enabled: false,
        };
        let is_drag = |action: &HotSwitchAction| match action {
            HotSwitchAction::Toggle(id) => id.starts_with("dnd."),
            HotSwitchAction::Group(ids) => ids.iter().any(|id| id.starts_with("dnd.")),
            HotSwitchAction::Preset(_) => true,
            HotSwitchAction::Panel => false,
        };
        let mut drag = vec![header(t!("hotswitch.section_drag").into())];
        let mut other = vec![header(t!("hotswitch.section_view").into())];
        for (binding, spec) in DEFAULT_BINDINGS.iter().enumerate() {
            let Some(name) = Self::binding_name(binding) else {
                continue;
            };
            let item = Item {
                label: name.into(),
                detail: SharedString::default(),
                keys: crate::hotswitch::keys_text(spec.keys).into(),
                run: Run::Switch(binding),
                enabled: true,
            };
            if is_drag(&spec.action) {
                drag.push(item);
            } else {
                other.push(item);
            }
        }
        drag.extend(other);
        drag
    }

    /// Whether a palette command can do something now.
    fn command_available(&self, id: &str) -> bool {
        let tab = self.tab();
        let folder = tab.location().as_path().is_some();
        let selected = folder && !tab.selection.is_empty();
        match id {
            "nav.back" => tab.history.can_go_back() || tab.search.is_some(),
            "nav.forward" => tab.history.can_go_forward(),
            "nav.up" => tab.location().parent().is_some(),
            "tab.next" | "tab.previous" => self.tabs.len() > 1,
            "search.filter" | "search.find" | "file.new_folder" | "sidebar.pin"
            | "file.open_new_tab" | "file.properties" => folder,
            "file.paste" => folder && sx_shell::clipboard::has_files(),
            "file.rename"
            | "file.delete"
            | "file.delete_permanent"
            | "file.copy"
            | "file.cut"
            | "file.copy_path" => selected,
            "edit.select_all" => tab.row_count() > 0,
            "edit.undo" => self.ops.undo.next_undo().is_some(),
            "edit.redo" => self.ops.undo.next_redo().is_some(),
            _ => true,
        }
    }

    /// Commands that are shown in the palette and implemented, the switches, then the places of
    /// the sidebar.
    fn palette_items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = crate::actions::palette_commands()
            .into_iter()
            .map(|(spec, action)| Item {
                label: t!(format!("command.{}", spec.id)).into(),
                detail: SharedString::default(),
                keys: spec
                    .bindings
                    .first()
                    .map(|keys| crate::hotswitch::keys_text(keys))
                    .unwrap_or_default()
                    .into(),
                enabled: self.command_available(spec.id),
                run: Run::Command(action, spec.context),
            })
            .collect();
        for (binding, spec) in DEFAULT_BINDINGS.iter().enumerate() {
            if matches!(spec.action, HotSwitchAction::Panel) {
                continue;
            }
            let Some(name) = Self::binding_name(binding) else {
                continue;
            };
            items.push(Item {
                label: name.into(),
                detail: SharedString::default(),
                keys: crate::hotswitch::keys_text(spec.keys).into(),
                run: Run::Switch(binding),
                enabled: true,
            });
        }
        let place = |label: SharedString, detail: String, location: Location| Item {
            label,
            detail: detail.into(),
            keys: SharedString::default(),
            run: Run::Go(location),
            enabled: true,
        };
        let place_word = t!("palette.place").into_owned();
        items.push(place(
            t!("nav.home").into(),
            place_word.clone(),
            Location::Home,
        ));
        items.push(place(
            t!("nav.this_pc").into(),
            place_word.clone(),
            Location::ThisPc,
        ));
        for drive in &self.drives {
            items.push(place(
                crate::format::drive_label(drive),
                drive.root.display().to_string(),
                Location::Path(drive.root.clone()),
            ));
        }
        for pin in &self.pins {
            items.push(place(
                pin.name.clone(),
                pin.path.display().to_string(),
                Location::Path(pin.path.clone()),
            ));
        }
        items.push(place(
            t!("nav.recycle_bin").into(),
            place_word,
            Location::RecycleBin,
        ));
        items
    }

    fn on_palette_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let text = input.read(cx).value();
                if let Some(palette) = &mut self.palette {
                    palette.filter(&text);
                    cx.notify();
                }
            }
            InputEvent::PressEnter { .. } => {
                let selected = self.palette.as_ref().map_or(0, |p| p.selected);
                self.run_palette(selected, window, cx);
            }
            InputEvent::Focus | InputEvent::Blur => {}
        }
    }

    fn palette_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let page = visible_rows(palette.mode, window, cx) as isize - 1;
        match event.keystroke.key.as_str() {
            "up" => palette.step(-1, true),
            "down" => palette.step(1, true),
            "pageup" => palette.step(-page, false),
            "pagedown" => palette.step(page, false),
            "escape" => self.close_palette(window, cx),
            // In the panel, Space switches the chosen one while nothing is typed (FEATURES.md
            // F-KEY-05); once a search is under way it is a space.
            "space"
                if palette.mode == PaletteMode::Switches
                    && !event.keystroke.modifiers.modified()
                    && palette.input.read(cx).value().is_empty() =>
            {
                let selected = palette.selected;
                self.run_palette(selected, window, cx);
            }
            // Focus stays in the palette while it is open.
            "tab" | "f6" => {}
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Closes the palette; focus goes back to where it was.
    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.take() else {
            return;
        };
        match palette.restore {
            Some(handle) => window.focus(&handle, cx),
            None => window.focus(&self.list_focus, cx),
        }
        cx.notify();
    }

    /// Runs the shown item at `index`. A disabled item does nothing and the palette stays.
    fn run_palette(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let Some(ix) = palette
            .shown
            .get(index)
            .map(|shown| shown.item)
            .filter(|&ix| palette.items.get(ix).is_some_and(|item| item.enabled))
        else {
            return;
        };
        if palette.mode == PaletteMode::Switches {
            if let Some(Run::Switch(binding)) = palette.items.get(ix).map(|item| &item.run) {
                let binding = *binding;
                self.run_switch_binding(binding, cx);
                cx.notify();
            }
            return;
        }
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let item = palette.items.swap_remove(ix);
        // List commands act on the list; the rest where focus was.
        let list = matches!(item.run, Run::Command(_, CommandContext::List));
        match &palette.restore {
            Some(handle) if !list => window.focus(handle, cx),
            _ => window.focus(&self.list_focus, cx),
        }
        match item.run {
            // Dispatched after the focus change, so it reaches the element that handles it.
            Run::Command(action, _) => window.defer(cx, move |window, cx| {
                window.dispatch_action(action, cx);
            }),
            Run::Switch(binding) => self.run_switch_binding(binding, cx),
            Run::Go(location) => self.navigate(location, cx),
            Run::Tab(id) => {
                if let Some(ix) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.activate_tab(ix, window, cx);
                }
            }
            Run::Header => {}
        }
        cx.notify();
    }

    pub(crate) fn render_palette(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let t = cx.tokens();
        let width = t
            .px(Token::SizePaletteWidth)
            .min(window.viewport_size().width - t.px(Token::SpaceXl) * 2.0);
        let panel = palette.mode == PaletteMode::Switches;
        let selected_label = palette
            .shown
            .get(palette.selected)
            .and_then(|shown| palette.items.get(shown.item))
            .map(|item| {
                if panel {
                    self.row_detail(item, true).spoken
                } else {
                    item.label.clone()
                }
            });
        let list = self.render_palette_list(palette, visible_rows(palette.mode, window, cx), cx);
        let title = palette_title(palette.mode);
        // The selected row is read out with the field, which keeps the focus; in the panel with
        // its state, so a switch turned there is heard too.
        let field_label: SharedString = match selected_label {
            Some(label) => format!("{title}: {label}").into(),
            None => title.clone(),
        };
        Some(
            div()
                .id("palette-scrim")
                .absolute()
                // Below the title bar, which keeps its drag area and window buttons.
                .top(t.px(Token::SizeTitlebar))
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .flex_col()
                .items_center()
                .pt(t.px(Token::SpaceMd))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(
                    div()
                        .id("palette")
                        .role(gpui_kit::Role::Dialog)
                        .aria_label(title)
                        .key_context(crate::actions::PALETTE_CONTEXT)
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .capture_key_down(cx.listener(Self::palette_key_down))
                        .flex()
                        .flex_col()
                        .w(width)
                        .rounded(t.px(Token::RadiusLg))
                        .bg(t.paint(Token::SurfaceOverlay))
                        .border_1()
                        .border_color(t.color(Token::BorderSubtle))
                        .shadow(t.shadow(Token::ShadowPopover))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(t.px(Token::SpaceSm))
                                .h(t.px(Token::SizeControl) + t.px(Token::SpaceSm))
                                .px(t.px(Token::SpaceMd))
                                // Over the overlay this is all but opaque: what is typed never
                                // sits on text from the window behind.
                                .bg(t.paint(Token::SurfaceInput))
                                .border_b_1()
                                .border_color(t.color(Token::Divider))
                                .child(
                                    Icon::new(IconName::Search)
                                        .size(t.px(Token::SizeIconSm))
                                        .text_color(t.color(Token::FgMuted)),
                                )
                                .child(
                                    Input::new(&palette.input)
                                        .appearance(false)
                                        .aria_label(field_label)
                                        .h_full(),
                                ),
                        )
                        .child(list)
                        .when(panel, |el| {
                            el.child(
                                div()
                                    .px(t.px(Token::SpaceMd))
                                    .py(t.px(Token::SpaceSm))
                                    .border_t_1()
                                    .border_color(t.color(Token::Divider))
                                    .text_size(t.px(Token::FontSizeCaption))
                                    .text_color(t.color(Token::FgMuted))
                                    // One line, so the panel keeps its height while the hint
                                    // changes; the field says it in full.
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.panel_hint(palette)),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// The rows, or what to say when nothing matches.
    fn render_palette_list(
        &self,
        palette: &Palette,
        visible: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.tokens();
        let row_height = t.px(Token::RowHeightDefault);
        let rows = palette.shown.len();
        if rows == 0 {
            div()
                .px(t.px(Token::SpaceLg))
                .py(t.px(Token::SpaceMd))
                .text_color(t.color(Token::FgMuted))
                .child(SharedString::from(t!(
                    match palette.mode {
                        PaletteMode::Commands | PaletteMode::Switches => "palette.empty",
                        PaletteMode::Folders => "palette.empty_folders",
                    },
                    query = crate::search::quoted_query(&palette.input.read(cx).value())
                )))
                .into_any_element()
        } else {
            div()
                .id("palette-list")
                .role(gpui_kit::Role::ListBox)
                .aria_label(palette_title(palette.mode))
                .relative()
                .h(row_height * rows.min(visible) as f32 + t.px(Token::SpaceXs) * 2.0)
                .child(
                    uniform_list(
                        "palette-rows",
                        rows,
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            this.render_palette_rows(range, cx)
                        }),
                    )
                    .track_scroll(&palette.scroll)
                    .size_full()
                    .p(t.px(Token::SpaceXs)),
                )
                .vertical_scrollbar(&palette.scroll)
                .into_any_element()
        }
    }

    fn render_palette_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(palette) = &self.palette else {
            return Vec::new();
        };
        range
            .filter_map(|ix| self.palette_row(palette, ix, cx))
            .collect()
    }

    fn palette_row(&self, palette: &Palette, ix: usize, cx: &Context<Self>) -> Option<AnyElement> {
        let t = cx.tokens();
        let shown = palette.shown.get(ix)?;
        let item = palette.items.get(shown.item)?;
        if matches!(item.run, Run::Header) {
            return Some(section_title(item.label.clone(), cx));
        }
        let selected = ix == palette.selected;
        let panel = palette.mode == PaletteMode::Switches;
        let tint = |token: Token| {
            if item.enabled {
                t.color(token)
            } else {
                t.color(Token::FgDisabled)
            }
        };
        let RowText {
            on: switch_on,
            detail,
            spoken,
        } = self.row_detail(item, panel);
        // In the panel, a row says its section, as titles are not read on their own.
        let group = panel.then(|| palette.group_of(shown.item)).flatten();
        let ranges = shown.positions.iter().map(|&at| {
            let len = item.label[at..].chars().next().map_or(1, char::len_utf8);
            at..at + len
        });
        let caption = |text: SharedString, token: Token| {
            div()
                .flex_none()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(t.px(Token::FontSizeCaption))
                .text_color(tint(token))
                .child(text)
        };
        // The panel ends every row with its toggle, so they line up (as in Windows Settings),
        // after the keys and a state column of one width; the palette ends with the keys.
        let keys = (!item.keys.is_empty()).then(|| caption(item.keys.clone(), Token::FgSecondary));
        let (keys_before, keys_after) = if panel { (keys, None) } else { (None, keys) };
        let state = if panel && switch_on.is_some() {
            Some(state_column(
                &detail,
                caption(detail.clone(), Token::FgMuted),
                cx,
            ))
        } else if detail.is_empty() {
            None
        } else {
            Some(caption(detail, Token::FgMuted).max_w(t.px(Token::SizePaletteWidth) * 0.4))
        };
        Some(
            row_frame(ix, selected, item.enabled, cx)
                .role(gpui_kit::Role::ListBoxOption)
                .aria_label(spoken)
                .aria_selected(selected)
                .aria_position_in_set(shown.place)
                .aria_size_of_set(palette.choices)
                .when_some(switch_on.filter(|_| panel), |el, on| {
                    el.aria_toggled(if on {
                        gpui_kit::Toggled::True
                    } else {
                        gpui_kit::Toggled::False
                    })
                })
                .when_some(
                    group,
                    gpui_kit::StatefulInteractiveElement::aria_description,
                )
                .text_color(tint(Token::FgPrimary))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.run_palette(ix, window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(crate::search::highlighted_ranges(
                            item.label.clone(),
                            ranges,
                            tint(Token::MatchFg),
                        )),
                )
                .children(keys_before)
                .children(state)
                .children(keys_after)
                .when_some(switch_on.filter(|_| panel), |el, on| {
                    el.child(toggle(on, item.enabled, cx))
                })
                .into_any_element(),
        )
    }
}

impl Workspace {
    /// The switch holding a binding back while it is on. A switch that is off is just off.
    fn held_by(&self, binding: usize) -> Option<&'static str> {
        if self.binding_on(binding) == Some(true) {
            self.binding_holder(binding)
        } else {
            None
        }
    }

    /// The panel's footer: why the chosen switch is on hold, or how the panel works.
    fn panel_hint(&self, palette: &Palette) -> SharedString {
        palette
            .shown
            .get(palette.selected)
            .and_then(|shown| palette.items.get(shown.item))
            .and_then(Item::run_switch)
            .and_then(|binding| self.held_by(binding))
            .map_or_else(
                || t!("hotswitch.panel_hint").into(),
                |holder| crate::hotswitch::held_hint(holder).into(),
            )
    }

    /// A row's switch state, read now (it can change while the palette is open), the text
    /// beside the label, and what a screen reader says for the row.
    fn row_detail(&self, item: &Item, panel: bool) -> RowText {
        let switch_on = item
            .run_switch()
            .and_then(|binding| self.binding_on(binding));
        let holder = if panel {
            item.run_switch().and_then(|binding| self.held_by(binding))
        } else {
            None
        };
        let detail = match (item.run_switch(), switch_on) {
            (Some(_), Some(_)) if holder.is_some() => t!("hotswitch.panel_blocked").into(),
            (Some(_), Some(true)) if panel => t!("hotswitch.state_on").into(),
            (Some(_), Some(false)) if panel => t!("hotswitch.state_off").into(),
            (Some(_), Some(true)) => t!("palette.switch_on").into(),
            (Some(_), Some(false)) => t!("palette.switch_off").into(),
            _ => item.detail.clone(),
        };
        let mut spoken = [item.label.as_ref(), detail.as_ref(), item.keys.as_ref()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        // Said in full; the row has room for the short form only.
        if let Some(holder) = holder {
            spoken = format!("{spoken}. {}", crate::hotswitch::held_hint(holder));
        }
        RowText {
            on: switch_on,
            detail,
            spoken: spoken.into(),
        }
    }
}

/// A palette row's box: selected, under the pointer, pressed (DESIGN.md §11).
fn row_frame(
    ix: usize,
    selected: bool,
    enabled: bool,
    cx: &gpui_kit::App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let t = cx.tokens();
    let (selected_bg, selected_border, hover, pressed) = (
        t.color(Token::RowSelectedBg),
        t.color(Token::RowSelectedBorder),
        t.color(Token::RowHoverBg),
        t.color(Token::PressedBg),
    );
    div()
        .id(ElementId::NamedInteger("palette-row".into(), ix as u64))
        .flex()
        .items_center()
        .gap(t.px(Token::SpaceMd))
        .w_full()
        .h(t.px(Token::RowHeightDefault))
        .px(t.px(Token::SpaceMd))
        .rounded(t.px(Token::RadiusSm))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .when(selected, |el| {
            el.bg(selected_bg).border_color(selected_border)
        })
        .when(!selected && enabled, |el| el.hover(move |s| s.bg(hover)))
        .when(enabled, |el| el.active(move |s| s.bg(pressed)))
}

/// What a row shows and says beside its label.
struct RowText {
    /// A switch's state; `None` for other rows.
    on: Option<bool>,
    detail: SharedString,
    spoken: SharedString,
}

impl Palette {
    /// The title of the section an item is in (the panel only).
    fn group_of(&self, item: usize) -> Option<SharedString> {
        self.items
            .get(..item)?
            .iter()
            .rev()
            .find(|item| matches!(item.run, Run::Header))
            .map(|title| title.label.clone())
    }
}

/// Rows the list shows before it scrolls. The panel shows all its switches when the window has
/// room (DESIGN.md §8.4); the other modes show [`VISIBLE_ROWS`].
fn visible_rows(mode: PaletteMode, window: &Window, cx: &gpui_kit::App) -> usize {
    if mode != PaletteMode::Switches {
        return VISIBLE_ROWS;
    }
    let t = cx.tokens();
    // The title bar and the gap above, the field, the footer, and a gap below.
    let around = t.px(Token::SizeTitlebar)
        + t.px(Token::SpaceMd) * 2.0
        + t.px(Token::SizeControl)
        + t.px(Token::SpaceSm)
        + t.px(Token::RowHeightDefault)
        + t.px(Token::SpaceXs) * 2.0;
    let room = (window.viewport_size().height - around) / t.px(Token::RowHeightDefault);
    // Whole rows only; never fewer than a few, never more than the panel can hold.
    (room.floor().max(0.0) as usize).clamp(MIN_PANEL_ROWS, MAX_PANEL_ROWS)
}

/// A switch's state word, as wide as the widest one so the keys before it line up. The other
/// words are laid out with no height: they give the width and are never seen or read.
fn state_column(shown: &SharedString, visible: gpui_kit::Div, cx: &gpui_kit::App) -> gpui_kit::Div {
    let t = cx.tokens();
    let words = [
        t!("hotswitch.state_on"),
        t!("hotswitch.state_off"),
        t!("hotswitch.panel_blocked"),
    ];
    div()
        .flex()
        .flex_col()
        .flex_none()
        .items_end()
        .child(visible)
        .child(
            div()
                .flex()
                .flex_col()
                .h(gpui_kit::px(0.0))
                .overflow_hidden()
                .text_size(t.px(Token::FontSizeCaption))
                .children(
                    words
                        .into_iter()
                        .filter(|word| word.as_ref() != shown.as_ref())
                        .map(|word| div().whitespace_nowrap().child(SharedString::from(word))),
                ),
        )
}

/// A section title in the Hot Switch panel: seen, never chosen. Screen readers hear it with
/// each row of its section instead (a list box holds options only).
fn section_title(label: SharedString, cx: &gpui_kit::App) -> AnyElement {
    let t = cx.tokens();
    div()
        .flex()
        .items_end()
        .w_full()
        .h(t.px(Token::RowHeightDefault))
        .px(t.px(Token::SpaceMd))
        .pb(t.px(Token::SpaceXs))
        .text_size(t.px(Token::FontSizeCaption))
        .text_color(t.color(Token::FgMuted))
        .child(label)
        .into_any_element()
}

impl Item {
    fn run_switch(&self) -> Option<usize> {
        match self.run {
            Run::Switch(binding) => Some(binding),
            _ => None,
        }
    }
}

fn palette_title(mode: PaletteMode) -> SharedString {
    match mode {
        PaletteMode::Commands => t!("palette.label").into(),
        PaletteMode::Folders => t!("palette.label_folders").into(),
        PaletteMode::Switches => t!("hotswitch.panel_title").into(),
    }
}

/// A switch's toggle (DESIGN.md §11): a pill with its thumb at the end that says the state.
/// Display only: the row is what is clicked.
fn toggle(on: bool, enabled: bool, cx: &gpui_kit::App) -> impl IntoElement {
    let t = cx.tokens();
    let height = t.px(Token::SizeToggleHeight);
    let thumb = height - t.px(Token::SpaceXs) * 2.0;
    div()
        .flex()
        .flex_none()
        .items_center()
        // Always a border, clear when on, so the thumb sits in the same place both ways.
        .border_1()
        .when(on, |el| {
            el.justify_end()
                .bg(t.paint(Token::ToggleOnBg))
                .border_color(gpui_kit::transparent_black())
        })
        .when(!on, |el| {
            el.justify_start()
                .border_color(t.color(Token::ToggleOffBorder))
        })
        .when(!enabled, |el| el.opacity(t.number(Token::OpacityDisabled)))
        .w(t.px(Token::SizeToggleWidth))
        .h(height)
        .px(t.px(Token::SpaceXs) - gpui_kit::px(crate::theme::HAIRLINE))
        .rounded(height / 2.0)
        .child(div().size(thumb).rounded(thumb / 2.0).bg(t.color(if on {
            Token::ToggleOnThumb
        } else {
            Token::ToggleOffThumb
        })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_palette_command_has_a_label_in_both_languages() {
        for (spec, _) in crate::actions::palette_commands() {
            let key = format!("command.{}", spec.id);
            for locale in ["en", "id"] {
                let label = rust_i18n::t!(&key, locale = locale);
                assert_ne!(label, key, "{key} has no {locale} label");
            }
        }
    }
}
