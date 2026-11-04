//! The search box (FEATURES.md F-SRCH-01): typing filters the current folder as you type, and
//! the matching part of each name shows in bold `match.fg`. `Ctrl+F` focuses the box, `Escape`
//! clears it and goes back to the list, `Down` and `Enter` go to the list. The filter belongs to
//! the tab: the box shows the active tab's filter, and going to another folder clears it. In a
//! narrow window (DESIGN.md §8.8) the box is an icon until it is used.

use std::ops::Range;

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
use sx_fs::ViewOptions;
use sx_theme::Token;

use crate::actions::WORKSPACE_CONTEXT;
use crate::focus::TabStop as _;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

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
                this.set_filter("", cx);
            }))
            .child(Icon::new(IconName::Close).size(icon * 0.75))
    }

    pub(crate) fn on_search_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let text = input.read(cx).value().to_string();
                self.search_shown = Some((self.tab().id, text.clone()));
                self.set_filter(&text, cx);
            }
            InputEvent::PressEnter { .. } => self.search_to_list(window, cx),
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
                self.set_filter("", cx);
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

    /// Filters the active tab's folder. The selection stays on the entries still shown.
    pub(crate) fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        if tab.filter == text {
            return;
        }
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

    /// Keeps the box in step with the active tab: its filter and "Search <folder>" (another
    /// tab, a folder change, a filter cleared from elsewhere). Called at the start of a frame;
    /// it compares what the box was last given, so a frame with nothing new costs no allocation.
    pub(crate) fn sync_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let value_differs = self
            .search_shown
            .as_ref()
            .is_none_or(|(id, text)| *id != tab.id || *text != tab.filter);
        let place_differs = self
            .search_place
            .as_ref()
            .is_none_or(|(id, location)| *id != tab.id || location != tab.location());
        if !value_differs && !place_differs {
            return;
        }
        let value = value_differs.then(|| tab.filter.clone());
        let placeholder: Option<SharedString> =
            place_differs.then(|| t!("search.placeholder", folder = tab.title()).into());
        let (id, location) = (tab.id, tab.location().clone());
        if let Some(value) = &value {
            self.search_shown = Some((id, value.clone()));
        }
        if place_differs {
            self.search_place = Some((id, location));
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
    fn long_queries_are_cut_for_messages() {
        assert_eq!(quoted_query("report"), "report");
        let long = "a".repeat(100);
        let cut = quoted_query(&long);
        assert_eq!(cut.chars().count(), 40);
        assert!(cut.ends_with('…'));
    }
}
