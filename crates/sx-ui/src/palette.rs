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

/// Folders remembered for quick open, most recent first (this session only, in memory).
pub(crate) const RECENT_FOLDERS: usize = 50;

/// What the palette offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteMode {
    /// Commands, switches and places (`Ctrl+Shift+P`).
    Commands,
    /// Folders to go to (`Ctrl+P`).
    Folders,
}

/// What an item does.
enum Run {
    Command(Box<dyn Action>, CommandContext),
    /// A Hot Switch binding, by index in `DEFAULT_BINDINGS`.
    Switch(usize),
    Go(Location),
    /// An open tab, by id: it is brought forward.
    Tab(u64),
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
}

pub(crate) struct Palette {
    mode: PaletteMode,
    input: Entity<InputState>,
    items: Vec<Item>,
    shown: Vec<Shown>,
    selected: usize,
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
                if let Some(found) = sx_search::fuzzy_match(query, &item.label) {
                    let shown = Shown {
                        item: ix,
                        positions: found.positions,
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
                };
                Some((false, found.score, shown))
            })
            .collect();
        if !query.trim().is_empty() {
            // Stable: equal keys keep the order of the list.
            scored.sort_by_key(|(label, score, _)| std::cmp::Reverse((*label, *score)));
        }
        self.shown = scored.into_iter().map(|(_, _, shown)| shown).collect();
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// One row up or down wraps around; a page stops at the ends.
    fn step(&mut self, delta: isize, wrap: bool) {
        let count = self.shown.len() as isize;
        if count == 0 {
            return;
        }
        let next = self.selected as isize + delta;
        let next = if wrap {
            next.rem_euclid(count)
        } else {
            next.clamp(0, count - 1)
        } as usize;
        self.selected = next;
        self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
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
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        let subscription = cx.subscribe_in(&input, window, Self::on_palette_event);
        input.update(cx, |input, cx| input.focus(window, cx));
        let items = match mode {
            PaletteMode::Commands => self.palette_items(),
            PaletteMode::Folders => self.folder_items(),
        };
        let mut palette = Palette {
            mode,
            input,
            items,
            shown: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            restore,
            _subscription: subscription,
        };
        palette.filter("");
        self.palette = Some(palette);
        cx.notify();
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
            let Some((name, _)) = self.binding_summary(binding) else {
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
        let page = VISIBLE_ROWS as isize - 1;
        match event.keystroke.key.as_str() {
            "up" => palette.step(-1, true),
            "down" => palette.step(1, true),
            "pageup" => palette.step(-page, false),
            "pagedown" => palette.step(page, false),
            "escape" => self.close_palette(window, cx),
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
        }
        cx.notify();
    }

    pub(crate) fn render_palette(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let t = cx.tokens();
        let width = t
            .px(Token::SizePaletteWidth)
            .min(window.viewport_size().width - t.px(Token::SpaceXl) * 2.0);
        let selected_label = palette
            .shown
            .get(palette.selected)
            .and_then(|shown| palette.items.get(shown.item))
            .map(|item| item.label.clone());
        let list = self.render_palette_list(palette, cx);
        let title: SharedString = match palette.mode {
            PaletteMode::Commands => t!("palette.label").into(),
            PaletteMode::Folders => t!("palette.label_folders").into(),
        };
        // The selected row is read out with the field, which keeps the focus.
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
                        .child(list),
                )
                .into_any_element(),
        )
    }

    /// The rows, or what to say when nothing matches.
    fn render_palette_list(&self, palette: &Palette, cx: &Context<Self>) -> AnyElement {
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
                        PaletteMode::Commands => "palette.empty",
                        PaletteMode::Folders => "palette.empty_folders",
                    },
                    query = crate::search::quoted_query(&palette.input.read(cx).value())
                )))
                .into_any_element()
        } else {
            div()
                .id("palette-list")
                .role(gpui_kit::Role::ListBox)
                .aria_label(match palette.mode {
                    PaletteMode::Commands => SharedString::from(t!("palette.label")),
                    PaletteMode::Folders => SharedString::from(t!("palette.label_folders")),
                })
                .relative()
                .h(row_height * rows.min(VISIBLE_ROWS) as f32 + t.px(Token::SpaceXs) * 2.0)
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
        let selected = ix == palette.selected;
        let (selected_bg, selected_border, hover, pressed) = (
            t.color(Token::RowSelectedBg),
            t.color(Token::RowSelectedBorder),
            t.color(Token::RowHoverBg),
            t.color(Token::PressedBg),
        );
        let tint = |token: Token| {
            if item.enabled {
                t.color(token)
            } else {
                t.color(Token::FgDisabled)
            }
        };
        let detail: SharedString = match item.run {
            // Read now: a switch can change while the palette is open.
            Run::Switch(binding) => match self.binding_summary(binding) {
                Some((_, true)) => t!("palette.switch_on").into(),
                Some((_, false)) => t!("palette.switch_off").into(),
                None => SharedString::default(),
            },
            _ => item.detail.clone(),
        };
        let ranges = shown.positions.iter().map(|&at| {
            let len = item.label[at..].chars().next().map_or(1, char::len_utf8);
            at..at + len
        });
        let spoken: SharedString = [item.label.as_ref(), detail.as_ref(), item.keys.as_ref()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
            .into();
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
        Some(
            div()
                .id(ElementId::NamedInteger("palette-row".into(), ix as u64))
                .role(gpui_kit::Role::ListBoxOption)
                .aria_label(spoken)
                .aria_selected(selected)
                .aria_position_in_set(ix + 1)
                .aria_size_of_set(palette.shown.len())
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
                .when(!selected && item.enabled, |el| {
                    el.hover(move |s| s.bg(hover))
                })
                .when(item.enabled, |el| el.active(move |s| s.bg(pressed)))
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
                .when(!detail.is_empty(), |el| {
                    el.child(
                        caption(detail, Token::FgMuted).max_w(t.px(Token::SizePaletteWidth) * 0.4),
                    )
                })
                .when(!item.keys.is_empty(), |el| {
                    el.child(caption(item.keys.clone(), Token::FgSecondary))
                })
                .into_any_element(),
        )
    }
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
