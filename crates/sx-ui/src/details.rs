//! The content area: the virtualised details view (FEATURES.md F-VIEW, DESIGN.md §11 "Baris
//! daftar") and the loading, empty and error states.

use std::ops::Range;

use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, ClickEvent, Context, Div, DragMoveEvent, ElementId, Empty, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Render, SharedString, Stateful, StatefulInteractiveElement as _, Styled, Window, div,
    uniform_list,
};
use rust_i18n::t;
use sx_core::{EntryKind, Location};
use sx_fs::Column;
use sx_theme::Token;

use crate::actions::{self, LIST_CONTEXT};
use crate::focus::{Ring, TabStop as _};
use crate::tab::Content;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Default column widths in px. They are user state (dragged in the header), not theme values;
/// Name takes the remaining width.
const COL_MODIFIED: f32 = 160.0;
const COL_TYPE: f32 = 140.0;
const COL_SIZE: f32 = 96.0;
/// How narrow or wide a column can be dragged.
const COL_MIN: f32 = 56.0;
const COL_MAX: f32 = 480.0;
/// Narrow content areas drop Type, then Modified (DESIGN.md §8.8): Name keeps at least room
/// for a readable file name next to the other columns.
const HIDE_TYPE_BELOW: f32 = 640.0;
const HIDE_MODIFIED_BELOW: f32 = 480.0;

/// The widths of a tab's fixed columns, as the user dragged them (per tab, for the session).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ColumnWidths {
    modified: f32,
    kind: f32,
    size: f32,
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            modified: COL_MODIFIED,
            kind: COL_TYPE,
            size: COL_SIZE,
        }
    }
}

impl ColumnWidths {
    fn get(&self, column: Column) -> Option<f32> {
        match column {
            Column::Modified => Some(self.modified),
            Column::Type => Some(self.kind),
            Column::Size => Some(self.size),
            Column::Name => None,
        }
    }

    fn set(&mut self, column: Column, width: f32) {
        let width = width.clamp(COL_MIN, COL_MAX);
        match column {
            Column::Modified => self.modified = width,
            Column::Type => self.kind = width,
            Column::Size => self.size = width,
            Column::Name => {}
        }
    }
}

/// A column border being dragged: which column, and where the pointer and its width started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ColumnResize {
    column: Column,
    start_x: f32,
    start_width: f32,
}

/// The drag payload of a column border; nothing follows the pointer.
struct ColumnDrag;

impl Render for ColumnDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy)]
struct Columns {
    modified: Option<Pixels>,
    kind: Option<Pixels>,
    size: Pixels,
}

impl Columns {
    fn for_width(width: Pixels, widths: &ColumnWidths) -> Self {
        let width = f32::from(width);
        Self {
            modified: (width >= HIDE_MODIFIED_BELOW).then(|| gpui_kit::px(widths.modified)),
            kind: (width >= HIDE_TYPE_BELOW).then(|| gpui_kit::px(widths.kind)),
            size: gpui_kit::px(widths.size),
        }
    }
}

impl Workspace {
    pub(crate) fn render_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let listing = matches!(&self.tab().content, Content::Folder(s) if s.visible_len() > 0);
        let body: AnyElement = match (&self.tab().content, self.tab().location()) {
            (Content::Page, Location::Home) => self.render_home(cx).into_any_element(),
            (Content::Page, Location::ThisPc) => self.render_this_pc(cx).into_any_element(),
            (Content::Page, _) => self.render_recycle_bin(cx).into_any_element(),
            (Content::Loading { since }, _) => {
                let text = (since.elapsed() >= crate::workspace::LOADING_DELAY)
                    .then(|| t!("details.loading").to_string());
                self.centered(text.map(SharedString::from), cx)
            }
            (Content::Failed(message), _) => self.render_failure(message.clone(), cx),
            (Content::Folder(snapshot), _) if snapshot.visible_len() == 0 => {
                self.centered(Some(t!("details.empty").into()), cx)
            }
            (Content::Folder(_), _) => self.render_details(window, cx).into_any_element(),
        };
        let t = cx.tokens();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(t.paint(Token::BgContent))
            .rounded_tl(t.px(Token::RadiusLg))
            .overflow_hidden()
            .children(self.notice.clone().map(|notice| {
                div()
                    .flex_none()
                    .px(t.px(Token::SpaceLg))
                    .py(t.px(Token::SpaceSm))
                    .text_color(t.color(Token::DangerText))
                    .child(notice)
            }))
            .map(|el| {
                if listing {
                    el.child(body)
                } else {
                    // Pages and loading, empty or failed folders hold the list's focus too, so
                    // F6 and a lost focus always have somewhere to land.
                    el.child(
                        div()
                            .id("content-focus")
                            .track_focus(&self.list_focus)
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(body),
                    )
                }
            })
    }

    fn centered(&self, text: Option<SharedString>, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .p(t.px(Token::Space2xl))
            .text_color(t.color(Token::FgMuted))
            .children(text)
            .into_any_element()
    }

    fn render_failure(&self, message: SharedString, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let can_up = self.tab().location().parent().is_some();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(t.px(Token::SpaceLg))
            .p(t.px(Token::Space2xl))
            .child(
                div()
                    .max_w(t.px(Token::SizeSidebar) * 2.0)
                    .text_color(t.color(Token::FgSecondary))
                    .child(message),
            )
            .when(can_up, |el| {
                el.child(
                    self.text_button("failure-up", t!("nav.go_up").into(), cx, |this, cx| {
                        this.go_up(cx);
                    }),
                )
            })
            .into_any_element()
    }

    /// A secondary button (DESIGN.md §11).
    pub(crate) fn text_button(
        &self,
        id: &'static str,
        label: SharedString,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let t = cx.tokens();
        let (hover, pressed, hover_fg) = (
            t.paint(Token::ButtonSecondaryBgHover),
            t.paint(Token::ButtonSecondaryBgPressed),
            t.color(Token::ButtonSecondaryFgHover),
        );
        let clear = gpui_kit::transparent_black();
        let focus = self.focus_stop(&ElementId::Name(id.into()), cx);
        div()
            .id(id)
            .tab_stop_ring(
                &focus,
                Ring::over(t.color(Token::ButtonSecondaryBorder)),
                cx,
            )
            .flex()
            .items_center()
            .h(t.px(Token::SizeControl))
            .px(t.px(Token::SpaceLg))
            .rounded(t.px(Token::RadiusSm))
            .bg(t.paint(Token::ButtonSecondaryBg))
            .text_color(t.color(Token::ButtonSecondaryFg))
            .hover(move |s| s.bg(hover).text_color(hover_fg).border_color(clear))
            .active(move |s| s.bg(pressed).text_color(hover_fg).border_color(clear))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
            .child(label)
    }

    fn render_details(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let sidebar = if self.sidebar_visible {
            t.px(Token::SizeSidebar)
        } else {
            gpui_kit::px(0.0)
        };
        let columns =
            Columns::for_width(window.viewport_size().width - sidebar, &self.tab().columns);
        let count = self.tab().row_count();
        let focused = self.list_focus.is_focused(window);
        let list = div()
            .id("details")
            .key_context(LIST_CONTEXT)
            .track_focus(&self.list_focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .on_key_down(cx.listener(Self::list_key_down));
        self.list_actions(list, cx)
            .child(self.render_header(columns, cx))
            .child(
                // The scrollbar overlays the rows below the header; its colours come through
                // the theme bridge (`scrollbar.*`).
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            "rows",
                            count,
                            cx.processor(move |this, range: Range<usize>, _window, cx| {
                                this.render_rows(range, columns, focused, cx)
                            }),
                        )
                        .track_scroll(&self.tab().scroll)
                        .flex_1()
                        .px(t.px(Token::SpaceXs))
                        .pb(t.px(Token::SpaceSm)),
                    )
                    .vertical_scrollbar(&self.tab().scroll),
            )
    }

    /// Keyboard selection and opening in the focused list.
    fn list_actions(&self, list: Stateful<Div>, cx: &Context<Self>) -> Stateful<Div> {
        list.on_action(cx.listener(|this, _: &actions::SelectPrevious, _, cx| {
            this.move_selection(-1, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::SelectNext, _, cx| {
            this.move_selection(1, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::SelectFirst, _, cx| {
            this.move_selection(isize::MIN / 2, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::SelectLast, _, cx| {
            this.move_selection(isize::MAX / 2, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::SelectPageUp, _, cx| {
            let page = this.tab().page_rows.get() as isize;
            this.move_selection(-page, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::SelectPageDown, _, cx| {
            let page = this.tab().page_rows.get() as isize;
            this.move_selection(page, false, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::ExtendPrevious, _, cx| {
            this.move_selection(-1, true, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::ExtendNext, _, cx| {
            this.move_selection(1, true, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::ExtendFirst, _, cx| {
            this.move_selection(isize::MIN / 2, true, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::ExtendLast, _, cx| {
            this.move_selection(isize::MAX / 2, true, false);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::MoveFocusPrevious, _, cx| {
            this.move_selection(-1, false, true);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::MoveFocusNext, _, cx| {
            this.move_selection(1, false, true);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::ToggleFocused, _, cx| {
            if let Some(focus) = this.tab().selection.focus() {
                this.tab_mut().selection.toggle(focus);
                cx.notify();
            }
        }))
        .on_action(cx.listener(|this, _: &actions::SelectAll, _, cx| {
            let len = this.tab().row_count() as u32;
            this.tab_mut().selection.select_all(len);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &actions::OpenSelected, _, cx| {
            this.open_selection(false, cx);
        }))
        .on_action(cx.listener(|this, _: &actions::OpenInNewTab, _, cx| {
            this.open_selection(true, cx);
        }))
    }

    fn render_header(&self, columns: Columns, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let sort = self.tab().sort;
        let hover = t.color(Token::HeaderHoverBg);
        let cell = |id: &'static str,
                    column: Column,
                    label: SharedString,
                    width: Option<Pixels>| {
            let arrow = (sort.column == column).then(|| {
                Icon::new(if sort.descending {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronUp
                })
                .size(t.px(Token::SizeIconSm) * 0.75)
            });
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(t.px(Token::SpaceXs))
                .h_full()
                .px(t.px(Token::SpaceSm))
                .rounded(t.px(Token::RadiusXs))
                .hover(move |s| s.bg(hover))
                .map(|el| match width {
                    Some(w) => el
                        .w(w)
                        .flex_none()
                        .relative()
                        .child(self.column_border(column, cx)),
                    None => el.flex_1().min_w_0(),
                })
                .when(column == Column::Size, Styled::justify_end)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.sort_by(column, cx)))
                .child(label)
                .children(arrow)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(t.px(Token::RowHeightCompact))
            .mx(t.px(Token::SpaceXs))
            .px(t.px(Token::SpaceSm))
            .text_size(t.px(Token::FontSizeCaption))
            .line_height(t.px(Token::FontLineCaption))
            .text_color(t.color(Token::HeaderFg))
            .border_b_1()
            .border_color(t.color(Token::Divider))
            .on_drag_move(cx.listener(Self::resize_column))
            .child(cell(
                "col-name",
                Column::Name,
                t!("details.name").into(),
                None,
            ))
            .when_some(columns.modified, |el, w| {
                el.child(cell(
                    "col-modified",
                    Column::Modified,
                    t!("details.modified").into(),
                    Some(w),
                ))
            })
            .when_some(columns.kind, |el, w| {
                el.child(cell(
                    "col-type",
                    Column::Type,
                    t!("details.type").into(),
                    Some(w),
                ))
            })
            .child(cell(
                "col-size",
                Column::Size,
                t!("details.size").into(),
                Some(columns.size),
            ))
    }

    /// The border at a fixed column's left edge: dragging it resizes that column; Name gives
    /// way.
    fn column_border(&self, column: Column, cx: &Context<Self>) -> Stateful<Div> {
        let t = cx.tokens();
        let (grip, divider) = (t.px(Token::SpaceSm), t.color(Token::Divider));
        div()
            .id(ElementId::NamedInteger("col-border".into(), column as u64))
            .absolute()
            .top_0()
            .bottom_0()
            .left(-grip / 2.0)
            .w(grip)
            .flex()
            .justify_center()
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    // Not a click on the column: no sort.
                    cx.stop_propagation();
                    let width = this.tab().columns.get(column).unwrap_or(COL_MIN);
                    this.column_resize = Some(ColumnResize {
                        column,
                        start_x: f32::from(event.position.x),
                        start_width: width,
                    });
                }),
            )
            .on_drag(ColumnDrag, |_, _, _, cx| cx.new(|_| ColumnDrag))
            .child(
                div()
                    .h_full()
                    .border_l_1()
                    .border_color(gpui_kit::transparent_black())
                    .hover(move |s| s.border_color(divider)),
            )
    }

    /// A column border moved: the column follows the pointer (left is wider).
    fn resize_column(
        &mut self,
        event: &DragMoveEvent<ColumnDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(resize) = self.column_resize else {
            return;
        };
        let width = resize.start_width + resize.start_x - f32::from(event.event.position.x);
        let columns = &mut self.tab_mut().columns;
        let before = *columns;
        columns.set(resize.column, width);
        if *columns != before {
            cx.notify();
        }
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        columns: Columns,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.tab()
            .page_rows
            .set(range.len().saturating_sub(1).max(1));
        let style = RowStyle::new(cx);
        let tab = self.tab();
        let focus_row = tab.selection.focus().map(|f| f as usize);
        range
            .filter_map(|row| {
                let state = RowState {
                    selected: tab.selection.contains(row as u32),
                    focused,
                    is_focus: focused && focus_row == Some(row),
                };
                let text = tab.row_text(row)?;
                let kind = self.row_kind(row)?;
                Some((row, kind, text, state))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(row, kind, text, state)| {
                let icon = self.system_icon(row, kind);
                self.render_row(row, kind, text, state, columns, icon, &style, cx)
            })
            .collect()
    }

    /// The system icon of a row: files always, folders only without themed folder icons
    /// (`view.themed_folder_icons`).
    fn system_icon(
        &mut self,
        row: usize,
        kind: EntryKind,
    ) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        let folder = kind.is_container();
        if folder && self.features.view_themed_folder_icons {
            return None;
        }
        let tab = self.tab();
        let snapshot = tab.snapshot()?;
        let entry = snapshot.row(row)?;
        let path = snapshot.path_of(tab.location().as_path()?, entry.index)?;
        self.icons.get(&path, folder)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        row: usize,
        kind: EntryKind,
        text: crate::format::RowText,
        state: RowState,
        columns: Columns,
        system_icon: Option<std::sync::Arc<gpui_kit::RenderImage>>,
        s: &RowStyle,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (icon, icon_color) = match kind {
            EntryKind::Dir | EntryKind::Junction => (IconName::Folder, s.folder),
            EntryKind::File | EntryKind::Symlink => (IconName::File, s.file),
        };
        let hover = s.hover;
        let cell = |width: Pixels| {
            div()
                .w(width)
                .flex_none()
                .px(s.space_sm)
                .text_color(s.secondary)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
        };
        div()
            .id(ElementId::NamedInteger("row".into(), row as u64))
            .flex()
            .items_center()
            .w_full()
            .h(s.height)
            .px(s.space_sm)
            .rounded(s.radius)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .when(state.selected && state.focused, |el| {
                el.bg(s.selected_bg).border_color(s.selected_border)
            })
            .when(state.selected && !state.focused, |el| el.bg(s.inactive_bg))
            .when(state.is_focus, |el| el.border_color(s.focus_ring))
            .when(!state.selected, |el| el.hover(move |st| st.bg(hover)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.list_focus, cx);
                let m = event.modifiers();
                if event.click_count() >= 2 && !m.control && !m.shift {
                    this.select_row(row, false, false);
                    this.open_row(row, false, cx);
                } else {
                    this.select_row(row, m.shift, m.control);
                }
                cx.notify();
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    if this.row_kind(row).is_some_and(EntryKind::is_container) {
                        this.open_row(row, true, cx);
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(s.space_sm)
                    .px(s.space_sm)
                    .child(match system_icon {
                        Some(image) => gpui_kit::img(image)
                            .size(s.icon)
                            .flex_none()
                            .into_any_element(),
                        None => Icon::new(icon)
                            .size(s.icon)
                            .flex_none()
                            .text_color(icon_color)
                            .into_any_element(),
                    })
                    .child(match self.rename_field(row, cx) {
                        Some(field) => field,
                        None => div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(text.name)
                            .into_any_element(),
                    }),
            )
            .when_some(columns.modified, |el, w| {
                el.child(cell(w).child(text.modified))
            })
            .when_some(columns.kind, |el, w| el.child(cell(w).child(text.kind)))
            .child(cell(columns.size).flex().justify_end().child(text.size))
            .into_any_element()
    }
}

impl Workspace {
    /// The inline rename field when `row` is the item being renamed (F-OPS-03). Escape
    /// cancels; Enter or leaving the field commits.
    fn rename_field(&self, row: usize, cx: &Context<Self>) -> Option<AnyElement> {
        let renaming = self.ops.renaming.as_ref()?;
        let tab = self.tab();
        let entry = tab.snapshot()?.row(row)?;
        if renaming.tab_id != tab.id || renaming.index != entry.index {
            return None;
        }
        let t = cx.tokens();
        Some(
            div()
                .flex_1()
                .min_w_0()
                .h(t.px(Token::RowHeightCompact))
                .px(t.px(Token::SpaceXs))
                .rounded(t.px(Token::RadiusXs))
                .bg(t.paint(Token::SurfaceInput))
                .border_1()
                .border_color(t.color(Token::BorderInputFocus))
                .capture_key_down(cx.listener(
                    |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            this.cancel_rename(window, cx);
                        }
                    },
                ))
                .child(Input::new(&renaming.input).appearance(false).h_full())
                .into_any_element(),
        )
    }
}

#[derive(Clone, Copy)]
struct RowState {
    selected: bool,
    /// The list has keyboard focus (active vs inactive selection colours).
    focused: bool,
    /// This row holds the keyboard focus.
    is_focus: bool,
}

/// Token values used by every row, read once per frame.
struct RowStyle {
    height: Pixels,
    hover: Hsla,
    selected_bg: Hsla,
    selected_border: Hsla,
    inactive_bg: Hsla,
    focus_ring: Hsla,
    secondary: Hsla,
    folder: Hsla,
    file: Hsla,
    icon: Pixels,
    space_sm: Pixels,
    radius: Pixels,
}

impl RowStyle {
    fn new(cx: &gpui_kit::App) -> Self {
        let t = cx.tokens();
        Self {
            height: t.px(Token::RowHeightDefault),
            hover: t.color(Token::RowHoverBg),
            selected_bg: t.color(Token::RowSelectedBg),
            selected_border: t.color(Token::RowSelectedBorder),
            inactive_bg: t.color(Token::RowSelectedInactiveBg),
            focus_ring: t.color(Token::FocusRing),
            secondary: t.color(Token::FgSecondary),
            folder: t.color(Token::IconFolderFront),
            file: t.color(Token::IconDefault),
            icon: t.px(Token::SizeIconSm),
            space_sm: t.px(Token::SpaceSm),
            radius: t.px(Token::RadiusSm),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_widths_stay_in_range_and_name_is_not_fixed() {
        let mut widths = ColumnWidths::default();
        widths.set(Column::Modified, 10.0);
        widths.set(Column::Size, 10_000.0);
        widths.set(Column::Name, 300.0);
        assert_eq!(widths.get(Column::Modified), Some(COL_MIN));
        assert_eq!(widths.get(Column::Size), Some(COL_MAX));
        assert_eq!(widths.get(Column::Type), Some(COL_TYPE));
        assert_eq!(widths.get(Column::Name), None);
    }
}
