//! Window chrome (DESIGN.md §8.1): the 40 px title bar holding the tabs and the window controls,
//! and the 44 px command bar with navigation buttons and the breadcrumb/address field.
//!
//! The title bar marks its empty area as `WindowControlArea::Drag` and the caption buttons as
//! `Min`/`Max`/`Close`, so Windows handles moving, double-click maximise and snap layouts. Tabs
//! and buttons `occlude()` the drag area below them so clicks reach them.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    WindowControlArea, div,
};
use rust_i18n::t;
use sx_core::Location;
use sx_theme::Token;

use crate::actions::WORKSPACE_CONTEXT;
use crate::focus::{self, TabStop as _};
use crate::format;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Segments kept at the end of a collapsed breadcrumb (plus the first one).
const BREADCRUMB_KEEP: usize = 3;

/// Caption buttons are 46 px wide on a 40 px title bar, as in Windows 11.
const CAPTION_WIDTH_RATIO: f32 = 1.15;

impl Workspace {
    pub(crate) fn render_title_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = cx.tokens();
        let height = t.px(Token::SizeTitlebar);
        let caption_w = height * CAPTION_WIDTH_RATIO;
        let icon = t.px(Token::SizeIconSm);
        let (fg, hover, danger, danger_fg) = (
            t.color(Token::FgPrimary),
            t.color(Token::HoverBg),
            t.color(Token::ButtonDangerBg),
            t.color(Token::ButtonDangerFg),
        );
        let caption = |id: &'static str, area: WindowControlArea, name: IconName| {
            let close = area == WindowControlArea::Close;
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(caption_w)
                .h_full()
                .text_color(fg)
                .window_control_area(area)
                .hover(move |s| {
                    if close {
                        s.bg(danger).text_color(danger_fg)
                    } else {
                        s.bg(hover)
                    }
                })
                .child(Icon::new(name).size(icon))
        };
        let maximize = if window.is_maximized() {
            IconName::WindowRestore
        } else {
            IconName::WindowMaximize
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(height)
            .bg(t.paint(Token::BgTitlebar))
            .child(
                div()
                    .id("titlebar-drag")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_end()
                    .gap(t.px(Token::SpaceXxs))
                    .pl(t.px(Token::SpaceMd))
                    .window_control_area(WindowControlArea::Drag)
                    .children(
                        (0..self.tabs.len()).map(|ix| self.render_tab(ix, cx).into_any_element()),
                    )
                    .child(self.render_new_tab_button(cx)),
            )
            .child(caption(
                "caption-min",
                WindowControlArea::Min,
                IconName::WindowMinimize,
            ))
            .child(caption("caption-max", WindowControlArea::Max, maximize))
            .child(caption(
                "caption-close",
                WindowControlArea::Close,
                IconName::WindowClose,
            ))
    }

    fn render_tab(&self, ix: usize, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let tab = &self.tabs[ix];
        let active = ix == self.active;
        let id = tab.id as usize;
        let (hover, active_fg, fg) = (
            t.color(Token::TabHoverBg),
            t.color(Token::TabActiveFg),
            t.color(Token::TabFg),
        );
        let control = t.px(Token::SizeControl);
        let element = ElementId::NamedInteger("tab".into(), id as u64);
        let focus = self.focus_stop(&element, cx);
        div()
            .id(element)
            .tab_stop_ring(&focus, Vec::new(), cx)
            .occlude()
            .relative()
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceSm))
            .h(control)
            .min_w(control * 3.0)
            .max_w(t.px(Token::SizeTabMax))
            .pl(t.px(Token::SpaceLg))
            .pr(t.px(Token::SpaceXs))
            .rounded_t(t.px(Token::RadiusMd))
            .when(active, |el| {
                el.bg(t.paint(Token::TabActiveBg)).text_color(active_fg)
            })
            .when(!active, |el| el.text_color(fg).hover(move |s| s.bg(hover)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.activate_tab(ix, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_tab(ix, window, cx)),
            )
            .child(
                Icon::new(IconName::Folder)
                    .size(t.px(Token::SizeIconSm))
                    .text_color(t.color(Token::IconFolderFront)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(tab.title()),
            )
            .child(
                div()
                    .id(ElementId::NamedInteger("tab-close".into(), id as u64))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(t.px(Token::SizeIconSm) + t.px(Token::SpaceSm))
                    .rounded(t.px(Token::RadiusXs))
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.close_tab(ix, window, cx);
                    }))
                    .child(Icon::new(IconName::Close).size(t.px(Token::SizeIconSm) * 0.75)),
            )
            .when(active, |el| {
                el.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left(t.px(Token::SpaceLg))
                        .right(t.px(Token::SpaceLg))
                        .h(t.px(Token::SizeTabIndicator))
                        .rounded(t.px(Token::RadiusXs))
                        .bg(t.paint(Token::TabActiveIndicator)),
                )
            })
    }

    fn render_new_tab_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let (hover, pressed, hover_fg) = (
            t.paint(Token::ButtonGhostBgHover),
            t.paint(Token::ButtonGhostBgPressed),
            t.color(Token::ButtonGhostFgHover),
        );
        let control = t.px(Token::SizeControl);
        let focus = self.focus_stop(&ElementId::Name("tab-new".into()), cx);
        div()
            .id("tab-new")
            .tab_stop_ring(&focus, Vec::new(), cx)
            .occlude()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(control)
            .mb(t.px(Token::SpaceXxs))
            .rounded(t.px(Token::RadiusSm))
            .text_color(t.color(Token::ButtonGhostFg))
            .hover(move |s| s.bg(hover).text_color(hover_fg))
            .active(move |s| s.bg(pressed).text_color(hover_fg))
            .tooltip(|window, cx| {
                Tooltip::new(SharedString::from(t!("tab.new")))
                    .action(&crate::actions::TabNew, Some(WORKSPACE_CONTEXT))
                    .build(window, cx)
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_tab(Location::Home, cx);
            }))
            .child(Icon::new(IconName::Plus).size(t.px(Token::SizeIconSm)))
    }

    pub(crate) fn render_command_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = cx.tokens();
        let tab = self.tab();
        let can_up = tab.location().parent().is_some();
        focus::area(div().id("command-bar"), &self.command_area)
            .flex()
            .flex_none()
            .items_center()
            .h(t.px(Token::SizeCommandbar))
            .px(t.px(Token::SpaceMd))
            .gap(t.px(Token::SpaceXs))
            .bg(t.paint(Token::BgWindow))
            .child(self.nav_button(
                "nav-back",
                IconName::ArrowLeft,
                tab.history.can_go_back(),
                cx,
                |this, cx| {
                    if this.tab_mut().history.back().is_some() {
                        this.load(cx);
                    }
                },
            ))
            .child(self.nav_button(
                "nav-forward",
                IconName::ArrowRight,
                tab.history.can_go_forward(),
                cx,
                |this, cx| {
                    if this.tab_mut().history.forward().is_some() {
                        this.load(cx);
                    }
                },
            ))
            .child(self.nav_button("nav-up", IconName::ArrowUp, can_up, cx, Workspace::go_up))
            .child(
                self.nav_button("nav-refresh", IconName::RefreshCw, true, cx, |this, cx| {
                    this.load(cx);
                    this.refresh_drives(cx);
                }),
            )
            .child(self.render_address(cx))
            .when(tab.location().as_path().is_some(), |el| {
                let pinned = tab.location().as_path().is_some_and(|p| self.is_pinned(p));
                el.child(self.nav_button(
                    "nav-pin",
                    if pinned {
                        IconName::StarFill
                    } else {
                        IconName::Star
                    },
                    true,
                    cx,
                    Workspace::toggle_pin,
                ))
            })
    }

    /// The name and shortcut shown over an icon button (icon-only controls need a text name).
    fn nav_tooltip(&self, id: &str) -> (SharedString, Option<Box<dyn gpui_kit::Action>>) {
        use crate::actions::{NavBack, NavForward, NavRefresh, NavUp, SidebarPin, TabNew};
        let pinned = self
            .tab()
            .location()
            .as_path()
            .is_some_and(|p| self.is_pinned(p));
        let (key, action): (&str, Box<dyn gpui_kit::Action>) = match id {
            "nav-back" => ("nav.back", Box::new(NavBack)),
            "nav-forward" => ("nav.forward", Box::new(NavForward)),
            "nav-up" => ("nav.up", Box::new(NavUp)),
            "nav-refresh" => ("nav.refresh", Box::new(NavRefresh)),
            "nav-pin" if pinned => ("nav.unpin", Box::new(SidebarPin)),
            "nav-pin" => ("nav.pin", Box::new(SidebarPin)),
            "tab-new" => ("tab.new", Box::new(TabNew)),
            _ => return (SharedString::default(), None),
        };
        (t!(key).into(), Some(action))
    }

    fn nav_button(
        &self,
        id: &'static str,
        icon: IconName,
        enabled: bool,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let t = cx.tokens();
        let (hover, pressed, hover_fg) = (
            t.paint(Token::ButtonGhostBgHover),
            t.paint(Token::ButtonGhostBgPressed),
            t.color(Token::ButtonGhostFgHover),
        );
        let (label, action) = self.nav_tooltip(id);
        let focus = self.focus_stop(&ElementId::Name(id.into()), cx);
        div()
            .id(id)
            .when(enabled, |el| el.tab_stop_ring(&focus, Vec::new(), cx))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(t.px(Token::SizeControl))
            .rounded(t.px(Token::RadiusSm))
            .text_color(t.color(Token::ButtonGhostFg))
            .tooltip(move |window, cx| {
                let tip = Tooltip::new(label.clone());
                match &action {
                    Some(action) => tip.action(action.as_ref(), Some(WORKSPACE_CONTEXT)),
                    None => tip,
                }
                .build(window, cx)
            })
            .when(!enabled, |el| el.opacity(t.number(Token::OpacityDisabled)))
            .when(enabled, |el| {
                el.hover(move |s| s.bg(hover).text_color(hover_fg))
                    .active(move |s| s.bg(pressed).text_color(hover_fg))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
            })
            .child(Icon::new(icon).size(t.px(Token::SizeIconSm)))
    }

    fn render_address(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let strong = t.color(Token::BorderInputFocus);
        let field = div()
            .id("address")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(t.px(Token::SizeControl))
            .ml(t.px(Token::SpaceSm))
            .px(t.px(Token::SpaceXs))
            .rounded(t.px(Token::RadiusSm))
            .bg(t.paint(Token::SurfaceInput))
            .border_1();
        if self.editing_address {
            return field
                .border_color(t.color(Token::BorderInputFocus))
                .child(Input::new(&self.address).appearance(false).h_full())
                .into_any_element();
        }
        field
            .border_color(t.color(Token::BorderInput))
            .hover(move |s| s.border_color(strong))
            .overflow_hidden()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.edit_address(window, cx);
            }))
            .children(self.breadcrumb(cx))
            .into_any_element()
    }

    /// `This PC › Local Disk (C:) › Users › Raka`; the last segment is the current folder.
    fn breadcrumb(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let t = cx.tokens();
        let mut segments: Vec<Option<(SharedString, Location)>> =
            self.segments().into_iter().map(Some).collect();
        if segments.len() > BREADCRUMB_KEEP + 1 {
            let tail = segments.split_off(segments.len() - BREADCRUMB_KEEP);
            segments.truncate(1);
            segments.push(None);
            segments.extend(tail);
        }
        let last = segments.len().saturating_sub(1);
        let (fg, current_fg, hover, separator) = (
            t.color(Token::BreadcrumbFg),
            t.color(Token::BreadcrumbCurrentFg),
            t.color(Token::BreadcrumbHoverBg),
            t.color(Token::BreadcrumbSeparator),
        );
        let mut out = Vec::with_capacity(segments.len() * 2);
        for (ix, segment) in segments.into_iter().enumerate() {
            if ix > 0 {
                out.push(
                    Icon::new(IconName::ChevronRight)
                        .size(t.px(Token::SizeIconSm))
                        .text_color(separator)
                        .flex_none()
                        .into_any_element(),
                );
            }
            let Some((label, location)) = segment else {
                // The collapsed middle: clicking it shows the whole path to edit.
                let focus = self.focus_stop(&ElementId::Name("crumb-more".into()), cx);
                out.push(
                    div()
                        .id("crumb-more")
                        .tab_stop_ring(&focus, Vec::new(), cx)
                        .flex_none()
                        .px(t.px(Token::SpaceSm))
                        .rounded(t.px(Token::RadiusXs))
                        .text_color(fg)
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.edit_address(window, cx);
                        }))
                        .child("…")
                        .into_any_element(),
                );
                continue;
            };
            let stop = match location.as_path() {
                Some(path) => format!("crumb:{}", path.display()),
                None => format!("crumb:{label}"),
            };
            let focus = self.focus_stop(&ElementId::Name(stop.into()), cx);
            out.push(
                div()
                    .id(ElementId::NamedInteger("crumb".into(), ix as u64))
                    .tab_stop_ring(&focus, Vec::new(), cx)
                    .flex_none()
                    .px(t.px(Token::SpaceSm))
                    .rounded(t.px(Token::RadiusXs))
                    .whitespace_nowrap()
                    .text_color(if ix == last { current_fg } else { fg })
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.navigate(location.clone(), cx);
                    }))
                    .child(label)
                    .into_any_element(),
            );
        }
        out
    }

    fn segments(&self) -> Vec<(SharedString, Location)> {
        let location = self.tab().location();
        let Some(path) = location.as_path() else {
            return vec![(crate::tab::location_title(location), location.clone())];
        };
        let mut chain: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
        chain.reverse();
        let mut out = Vec::with_capacity(chain.len() + 1);
        if is_drive_path(path) {
            out.push((t!("nav.this_pc").into(), Location::ThisPc));
        }
        for (ix, step) in chain.into_iter().enumerate() {
            let label: SharedString = if ix == 0 {
                self.root_label(&step)
            } else {
                step.file_name()
                    .map_or_else(
                        || step.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                    .into()
            };
            out.push((label, Location::Path(step)));
        }
        out
    }

    /// `Local Disk (C:)` for drive roots, the share for UNC roots.
    fn root_label(&self, root: &Path) -> SharedString {
        let letter = root
            .to_str()
            .and_then(|s| s.chars().next())
            .map(|c| c.to_ascii_uppercase());
        self.drives
            .iter()
            .find(|d| Some(d.letter) == letter && is_drive_path(root))
            .map_or_else(|| root.display().to_string().into(), format::drive_label)
    }
}

fn is_drive_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(p))
            if matches!(p.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))
    )
}
