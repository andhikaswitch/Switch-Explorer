//! The content area: the virtualised details view (FEATURES.md F-VIEW, DESIGN.md §11 "Baris
//! daftar") and the loading, empty and error states.

use std::ops::Range;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    uniform_list,
};
use rust_i18n::t;
use sx_core::{EntryKind, Location};
use sx_fs::Column;
use sx_theme::Token;

use crate::actions::{self, LIST_CONTEXT};
use crate::tab::Content;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Default column widths in px. They are per-folder user state (resizable later), not theme
/// values; Name takes the remaining width.
const COL_MODIFIED: f32 = 160.0;
const COL_TYPE: f32 = 140.0;
const COL_SIZE: f32 = 96.0;
/// Narrow windows drop Type, then Modified (DESIGN.md §8.8).
const HIDE_TYPE_BELOW: f32 = 760.0;
const HIDE_MODIFIED_BELOW: f32 = 600.0;

#[derive(Clone, Copy)]
struct Columns {
    modified: Option<Pixels>,
    kind: Option<Pixels>,
    size: Pixels,
}

impl Columns {
    fn for_width(width: Pixels) -> Self {
        let width = f32::from(width);
        Self {
            modified: (width >= HIDE_MODIFIED_BELOW).then(|| gpui_kit::px(COL_MODIFIED)),
            kind: (width >= HIDE_TYPE_BELOW).then(|| gpui_kit::px(COL_TYPE)),
            size: gpui_kit::px(COL_SIZE),
        }
    }
}

impl Workspace {
    pub(crate) fn render_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let body: AnyElement = match (&self.tab().content, self.tab().location()) {
            (Content::Page, Location::Home) => self.render_home(cx).into_any_element(),
            (Content::Page, Location::ThisPc) => self.render_this_pc(cx).into_any_element(),
            (Content::Page, _) => self.render_recycle_bin(cx).into_any_element(),
            (Content::Loading { since }, _) => {
                let text = (since.elapsed() >= std::time::Duration::from_millis(150))
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
            .child(body)
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
                    self.text_button("failure-up", t!("nav.up").into(), cx, |this, cx| {
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
        let (hover, pressed) = (
            t.color(Token::ButtonSecondaryBgHover),
            t.color(Token::PressedBg),
        );
        div()
            .id(id)
            .flex()
            .items_center()
            .h(t.px(Token::SizeControl))
            .px(t.px(Token::SpaceLg))
            .rounded(t.px(Token::RadiusSm))
            .bg(t.paint(Token::ButtonSecondaryBg))
            .border_1()
            .border_color(t.color(Token::ButtonSecondaryBorder))
            .text_color(t.color(Token::ButtonSecondaryFg))
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
            .child(label)
    }

    fn render_details(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let columns = Columns::for_width(window.viewport_size().width);
        let count = self.tab().row_count();
        let focused = self.list_focus.is_focused(window);
        div()
            .id("details")
            .key_context(LIST_CONTEXT)
            .track_focus(&self.list_focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .on_key_down(cx.listener(Self::list_key_down))
            .on_action(cx.listener(|this, _: &actions::SelectPrevious, _, cx| {
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
            .child(self.render_header(columns, cx))
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
                    Some(w) => el.w(w).flex_none(),
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
                Some(self.render_row(row, kind, text, state, columns, &style, cx))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        row: usize,
        kind: EntryKind,
        text: crate::format::RowText,
        state: RowState,
        columns: Columns,
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
            .when(state.is_focus && !state.selected, |el| {
                el.border_color(s.focus_ring)
            })
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
                    .child(
                        Icon::new(icon)
                            .size(s.icon)
                            .flex_none()
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(text.name),
                    ),
            )
            .when_some(columns.modified, |el, w| {
                el.child(cell(w).child(text.modified))
            })
            .when_some(columns.kind, |el, w| el.child(cell(w).child(text.kind)))
            .child(cell(columns.size).flex().justify_end().child(text.size))
            .into_any_element()
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
