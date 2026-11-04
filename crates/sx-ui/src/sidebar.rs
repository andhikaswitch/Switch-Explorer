//! Sidebar (FEATURES.md F-SIDE-01, F-SIDE-06): Home, Pinned, This PC with a gauge per drive,
//! Recycle Bin. The active item is the location closest to the current folder.

use std::path::Path;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    ClickEvent, Context, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, relative,
};
use rust_i18n::t;
use sx_core::Location;
use sx_theme::Token;
use sx_win::Drive;

use crate::focus::{self, TabStop as _};
use crate::format;
use crate::slash;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Lucide "trash" from our own assets (`IconName::Delete` is a backspace glyph).
const TRASH_ICON: &str = "icons/sx/trash.svg";

/// Gauge colours switch at these fill levels (F-SIDE-06).
const GAUGE_WARNING: f32 = 0.85;
const GAUGE_DANGER: f32 = 0.95;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Home,
    Pin(usize),
    ThisPc,
    Drive(char),
    RecycleBin,
}

impl Workspace {
    /// The sidebar item that represents the current location best.
    fn active_item(&self) -> Option<Item> {
        match self.tab().location() {
            Location::Home => Some(Item::Home),
            Location::ThisPc => Some(Item::ThisPc),
            Location::RecycleBin => Some(Item::RecycleBin),
            Location::Path(path) => {
                let pin = self
                    .pins
                    .iter()
                    .enumerate()
                    .filter(|(_, pin)| path.starts_with(&pin.path))
                    .max_by_key(|(_, pin)| pin.path.as_os_str().len())
                    .map(|(ix, _)| Item::Pin(ix));
                pin.or_else(|| drive_letter(path).map(Item::Drive))
            }
        }
    }

    pub(crate) fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let active = self.active_item();
        let item = |id: Item, icon: IconName, label: SharedString, to: Location| {
            self.sidebar_item(id, icon, label, None, active == Some(id), to, cx)
        };
        focus::area(div().id("sidebar"), &self.sidebar_area)
            .flex()
            .flex_col()
            .flex_none()
            .w(t.px(Token::SizeSidebar))
            .h_full()
            .px(t.px(Token::SpaceMd))
            .py(t.px(Token::SpaceSm))
            .gap(t.px(Token::SpaceXxs))
            .bg(t.paint(Token::BgSidebar))
            .overflow_y_scroll()
            .child(item(
                Item::Home,
                IconName::LayoutDashboard,
                t!("nav.home").into(),
                Location::Home,
            ))
            .child(self.section(t!("nav.pinned").into(), cx))
            .children(self.pins.iter().enumerate().map(|(ix, pin)| {
                item(
                    Item::Pin(ix),
                    IconName::Folder,
                    pin.name.clone(),
                    Location::Path(pin.path.clone()),
                )
            }))
            .child(self.section(t!("nav.drives").into(), cx))
            .child(item(
                Item::ThisPc,
                IconName::HardDrive,
                t!("nav.this_pc").into(),
                Location::ThisPc,
            ))
            .children(self.drives.iter().map(|drive| {
                let id = Item::Drive(drive.letter);
                self.sidebar_item(
                    id,
                    IconName::HardDrive,
                    format::drive_label(drive),
                    Some(drive),
                    active == Some(id),
                    Location::Path(drive.root.clone()),
                    cx,
                )
            }))
            .child(div().h(t.px(Token::SpaceMd)))
            .child(item(
                Item::RecycleBin,
                IconName::Delete,
                t!("nav.recycle_bin").into(),
                Location::RecycleBin,
            ))
    }

    fn section(&self, label: SharedString, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        div()
            .pt(t.px(Token::SpaceLg))
            .pb(t.px(Token::SpaceXs))
            .px(t.px(Token::SpaceMd))
            .text_size(t.px(Token::FontSizeCaption))
            .line_height(t.px(Token::FontLineCaption))
            .text_color(t.color(Token::NavSectionFg))
            .child(label)
    }

    /// The capacity bar of a drive item. The normal fill is the highlight gradient, which would
    /// vanish on a highlighted item, so there it takes the item's text colour; warning and danger
    /// keep their colours.
    fn drive_gauge(&self, drive: &Drive, highlighted: bool, cx: &Context<Self>) -> Option<Div> {
        let t = cx.tokens();
        let used = drive.space?.used_fraction();
        let fill = if used >= GAUGE_DANGER {
            Token::GaugeFillDanger
        } else if used >= GAUGE_WARNING {
            Token::GaugeFillWarning
        } else {
            Token::GaugeFill
        };
        let normal = fill == Token::GaugeFill;
        let on_highlight = t.color(Token::NavItemActiveFg);
        Some(
            div()
                .mt(t.px(Token::SpaceXxs))
                .h(t.px(Token::SpaceXs))
                .w_full()
                .rounded(t.px(Token::RadiusXs))
                .bg(t.color(Token::GaugeTrack))
                .child(
                    div()
                        .h_full()
                        .w(relative(used))
                        .rounded(t.px(Token::RadiusXs))
                        .map(|bar| {
                            if normal && highlighted {
                                bar.bg(on_highlight)
                            } else {
                                bar.bg(t.paint(fill))
                            }
                        }),
                ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn sidebar_item(
        &self,
        id: Item,
        icon: IconName,
        label: SharedString,
        drive: Option<&Drive>,
        active: bool,
        to: Location,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let t = cx.tokens();
        let element_id = id.element_id();
        let (fg, active_fg, icon_fg, muted) = (
            t.color(Token::NavItemFg),
            t.color(Token::NavItemActiveFg),
            t.color(Token::NavItemIcon),
            t.color(Token::FgMuted),
        );
        let (hover, pressed, hover_fg) = (
            t.paint(Token::NavItemHoverBg),
            t.paint(Token::NavItemPressedBg),
            t.color(Token::NavItemHoverFg),
        );
        let lit = !active && self.hovered.as_ref() == Some(&element_id);
        let gauge = drive.and_then(|d| self.drive_gauge(d, active || lit, cx));
        let detail = drive.map(format::drive_space);
        let control = t.px(Token::SizeControl);
        let glow = if active { active_glow(cx) } else { Vec::new() };
        // A pin's stop follows its folder, so unpinning another one does not move the focus.
        let focus_key = match (&id, &to) {
            (Item::Pin(_), Location::Path(path)) => {
                ElementId::Name(format!("side-pin:{}", path.display()).into())
            }
            _ => element_id.clone(),
        };
        let focus = self.focus_stop(&focus_key, cx);
        div()
            .id(element_id.clone())
            .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceMd))
            .min_h(control)
            .px(t.px(Token::SpaceMd))
            .py(t.px(Token::SpaceXs))
            .rounded(t.px(Token::RadiusSm))
            .relative()
            .when(active, |el| {
                el.text_color(active_fg)
                    // No-op hover/active styles keep GPUI tracking the pointer here; else the
                    // item comes back from active with stale hover text (dark on the sidebar).
                    .hover(std::convert::identity)
                    .active(std::convert::identity)
                    .shadow(glow)
                    .child(active_slash(cx))
            })
            .when(!active, |el| {
                el.text_color(if lit { hover_fg } else { fg })
                    .when(lit, |el| el.bg(hover))
                    .hover(move |s| s.bg(hover).text_color(hover_fg))
                    .active(move |s| s.bg(pressed).text_color(hover_fg))
            })
            .on_hover(cx.listener({
                let id = element_id.clone();
                move |this, hovered: &bool, _, cx| this.set_hovered(&id, *hovered, cx)
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.navigate(to.clone(), cx);
            }))
            .child(
                div()
                    .flex_none()
                    .when(!active, |d| {
                        d.text_color(if lit { hover_fg } else { icon_fg })
                    })
                    .child(
                        Icon::new(icon)
                            .when(matches!(id, Item::RecycleBin), |i| i.path(TRASH_ICON))
                            .size(t.px(Token::SizeIconSm)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .children(gauge)
                    .children(detail.map(|text| {
                        div()
                            .text_size(t.px(Token::FontSizeCaption))
                            .line_height(t.px(Token::FontLineCaption))
                            .when(!active, |el| {
                                el.text_color(if lit { hover_fg } else { muted })
                            })
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(text)
                    })),
            )
    }
}

fn drive_letter(path: &Path) -> Option<char> {
    match path.components().next()? {
        std::path::Component::Prefix(prefix) => match prefix.kind() {
            std::path::Prefix::Disk(l) | std::path::Prefix::VerbatimDisk(l) => {
                Some(char::from(l).to_ascii_uppercase())
            }
            _ => None,
        },
        _ => None,
    }
}

impl Item {
    fn element_id(self) -> ElementId {
        match self {
            Self::Home => ElementId::Name("side-home".into()),
            Self::Pin(ix) => ElementId::NamedInteger("side-pin".into(), ix as u64),
            Self::ThisPc => ElementId::Name("side-pc".into()),
            Self::Drive(letter) => ElementId::NamedInteger("side-drive".into(), u64::from(letter)),
            Self::RecycleBin => ElementId::Name("side-bin".into()),
        }
    }
}

/// The Tide Slash behind the active item (DESIGN.md §7).
fn active_slash(cx: &gpui_kit::App) -> impl IntoElement {
    let t = cx.tokens();
    slash::tide_slash(slash::Slash {
        fill: t.resolved().paint(Token::NavItemActiveBg).clone(),
        edge: t.resolved().color(Token::NavItemActiveEdge),
        angle_deg: t.number(Token::SlashAngleDeg),
        radius: t.number(Token::RadiusSm),
        edge_width: t.number(Token::SlashEdgeWidth),
    })
}

/// The glow under the active item's Tide Slash (`shadow.glow` in `nav.item.active.glow`).
fn active_glow(cx: &gpui_kit::App) -> Vec<gpui_kit::BoxShadow> {
    let t = cx.tokens();
    let mut glow = t.shadow(Token::ShadowGlow);
    for shadow in &mut glow {
        shadow.color = t.color(Token::NavItemActiveGlow);
    }
    glow
}
