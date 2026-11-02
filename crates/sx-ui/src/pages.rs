//! Pages that are not folder listings: Home (placeholder in Phase 1), This PC and Recycle Bin
//! (opened in File Explorer until its own view lands, ROADMAP Phase 2).

use std::path::PathBuf;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, relative,
};
use rust_i18n::t;
use sx_core::Location;
use sx_theme::Token;

use crate::format;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

impl Workspace {
    fn page(&self, title: SharedString, cx: &Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let t = cx.tokens();
        div()
            .id("page")
            .flex()
            .flex_col()
            .flex_1()
            .gap(t.px(Token::SpaceLg))
            .p(t.px(Token::Space2xl))
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(t.px(Token::FontSizeTitle))
                    .line_height(t.px(Token::FontLineTitle))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title),
            )
    }

    pub(crate) fn render_home(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        self.page(t!("home.title").into(), cx).child(
            div()
                .text_color(t.color(Token::FgSecondary))
                .child(SharedString::from(t!("home.body"))),
        )
    }

    pub(crate) fn render_recycle_bin(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        self.page(t!("recycle_bin.title").into(), cx)
            .child(
                div()
                    .text_color(t.color(Token::FgSecondary))
                    .child(SharedString::from(t!("recycle_bin.body"))),
            )
            .child(div().flex().child(self.text_button(
                "bin-open",
                t!("recycle_bin.open").into(),
                cx,
                |this, cx| this.open_shell_folder("shell:RecycleBinFolder", cx),
            )))
    }

    pub(crate) fn render_this_pc(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let (hover, hover_fg) = (
            t.paint(Token::SurfaceCardHover),
            t.color(Token::CardHoverFg),
        );
        let tile_w = t.px(Token::SizeSidebar);
        self.page(t!("this_pc.title").into(), cx).child(
            div()
                .flex()
                .flex_wrap()
                .gap(t.px(Token::SpaceLg))
                .children(self.drives.iter().map(|drive| {
                    let used = drive.space.map_or(0.0, sx_win::Space::used_fraction);
                    let root = drive.root.clone();
                    let id = ElementId::NamedInteger("pc-drive".into(), u64::from(drive.letter));
                    let lit = self.hovered.as_ref() == Some(&id);
                    let tint = |normal| if lit { hover_fg } else { normal };
                    div()
                        .id(id.clone())
                        .flex()
                        .gap(t.px(Token::SpaceLg))
                        .w(tile_w)
                        .p(t.px(Token::SpaceLg))
                        .rounded(t.px(Token::RadiusLg))
                        .bg(t.paint(Token::SurfaceCard))
                        .shadow(t.shadow(Token::ShadowCard))
                        .when(lit, |el| el.bg(hover).text_color(hover_fg))
                        .hover(move |s| s.bg(hover).text_color(hover_fg))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            this.set_hovered(&id, *hovered, cx);
                        }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.navigate(Location::Path(root.clone()), cx);
                        }))
                        .child(
                            div()
                                .flex_none()
                                .text_color(tint(t.color(Token::NavItemIcon)))
                                .child(Icon::new(IconName::HardDrive).size(t.px(Token::SizeIcon))),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(t.px(Token::SpaceXs))
                                .child(format::drive_label(drive))
                                .child(
                                    div()
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
                                                    if lit {
                                                        bar.bg(hover_fg)
                                                    } else {
                                                        bar.bg(t.paint(Token::GaugeFill))
                                                    }
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(t.px(Token::FontSizeCaption))
                                        .line_height(t.px(Token::FontLineCaption))
                                        .text_color(tint(t.color(Token::FgSecondary)))
                                        .child(format::drive_space(drive)),
                                ),
                        )
                })),
        )
    }

    /// Opens a shell location (`shell:RecycleBinFolder`) in File Explorer.
    fn open_shell_folder(&mut self, name: &str, cx: &mut Context<Self>) {
        let open = sx_shell::exec::open(PathBuf::from(name), None);
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            if let Err(err) = open.await
                && !err.is_cancelled()
            {
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(crate::format::open_error(&name, &err));
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
