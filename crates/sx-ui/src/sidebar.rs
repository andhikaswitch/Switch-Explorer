//! Sidebar (FEATURES.md F-SIDE-01, F-SIDE-06): Home, Pinned, This PC with a gauge per drive,
//! Recycle Bin. The active item is the location closest to the current folder.

use std::path::Path;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, relative,
};
use rust_i18n::t;
use sx_core::Location;
use sx_theme::Token;
use sx_win::Drive;

use crate::format;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

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
        div()
            .id("sidebar")
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
        let element_id = match id {
            Item::Home => ElementId::Name("side-home".into()),
            Item::Pin(ix) => ElementId::NamedInteger("side-pin".into(), ix as u64),
            Item::ThisPc => ElementId::Name("side-pc".into()),
            Item::Drive(letter) => ElementId::NamedInteger("side-drive".into(), u64::from(letter)),
            Item::RecycleBin => ElementId::Name("side-bin".into()),
        };
        let (fg, active_fg, hover, icon_fg) = (
            t.color(Token::NavItemFg),
            t.color(Token::NavItemActiveFg),
            t.color(Token::NavItemHoverBg),
            t.color(Token::NavItemIcon),
        );
        let gauge = drive.and_then(|d| d.space).map(|space| {
            let used = space.used_fraction();
            let fill = if used >= GAUGE_DANGER {
                Token::GaugeFillDanger
            } else if used >= GAUGE_WARNING {
                Token::GaugeFillWarning
            } else {
                Token::GaugeFill
            };
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
                        .bg(t.paint(fill)),
                )
        });
        let detail = drive.map(format::drive_space);
        let control = t.px(Token::SizeControl);
        div()
            .id(element_id)
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceMd))
            .min_h(control)
            .px(t.px(Token::SpaceMd))
            .py(t.px(Token::SpaceXs))
            .rounded(t.px(Token::RadiusSm))
            .when(active, |el| {
                el.bg(t.paint(Token::NavItemActiveBg))
                    .text_color(active_fg)
                    .shadow(t.shadow(Token::ShadowGlow))
            })
            .when(!active, |el| el.text_color(fg).hover(move |s| s.bg(hover)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.navigate(to.clone(), cx);
            }))
            .child(
                Icon::new(icon)
                    .size(t.px(Token::SizeIconSm))
                    .flex_none()
                    .when(!active, |i| i.text_color(icon_fg)),
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
                            .when(!active, |el| el.text_color(t.color(Token::FgMuted)))
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
