//! Status bar (DESIGN.md §8.1): item count, selection with its size, free space of the drive.

use gpui_kit::{Context, IntoElement, ParentElement as _, SharedString, Styled as _, div};
use rust_i18n::t;
use sx_theme::Token;

use crate::format;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

impl Workspace {
    pub(crate) fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let tab = self.tab();
        let mut left: Vec<SharedString> = Vec::new();
        if let Some(snapshot) = tab.snapshot() {
            left.push(format::item_count(snapshot.visible_len()));
            let (count, bytes) = tab.selection_summary();
            if count > 0 {
                let mut text = format::selected_count(count).to_string();
                if bytes > 0 {
                    text.push_str(" (");
                    text.push_str(&sx_win::format::byte_size(bytes));
                    text.push(')');
                }
                left.push(text.into());
            }
        }
        let free = tab.location().as_path().and_then(|path| {
            let letter = path
                .to_str()
                .and_then(|s| s.chars().next())
                .map(|c| c.to_ascii_uppercase())?;
            let drive = self.drives.iter().find(|d| d.letter == letter)?;
            let space = drive.space?;
            Some(SharedString::from(t!(
                "status.free",
                drive = format!("{letter}:"),
                free = sx_win::format::byte_size(space.free)
            )))
        });
        let gap = t.px(Token::SpaceXl);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(gap)
            .h(t.px(Token::SizeStatusbar))
            .px(t.px(Token::SpaceLg))
            .bg(t.paint(Token::BgStatusbar))
            .text_size(t.px(Token::FontSizeCaption))
            .line_height(t.px(Token::FontLineCaption))
            .text_color(t.color(Token::FgSecondary))
            .children(left.into_iter().map(|text| div().child(text)))
            .children(free.map(|text| div().child(text)))
            .child(div().flex_1())
            .children(self.render_drag_chip(cx))
    }
}
