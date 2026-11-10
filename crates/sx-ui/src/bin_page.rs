//! The Recycle Bin page (FEATURES.md F-LOC-02): what is in the bin of every drive, with Restore,
//! Delete permanently and Empty. Restore and delete run through `sx-ops` (journal; a restore can
//! be undone), and the permanent ones ask first.

use std::collections::BTreeSet;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, div, uniform_list,
};
use rust_i18n::t;
use sx_core::Location;
use sx_ops::{Binned, Op};
use sx_shell::recycle::BinItem;
use sx_theme::Token;

use crate::focus::TabStop as _;
use crate::ops::{Dialog, Origin};
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Where the last listing got to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Listed {
    #[default]
    NotYet,
    Shown,
    Failed,
}

/// What the page shows.
#[derive(Debug, Default)]
pub(crate) struct BinView {
    /// Newest deletion first.
    items: Vec<BinItem>,
    /// Indexes into `items`.
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    loading: bool,
    /// Something changed while a listing ran: list again when it ends.
    dirty: bool,
    listed: Listed,
    /// Items the listing left out (no readable or plausible record).
    skipped: usize,
    /// The newest listing asked for; an older one is dropped.
    generation: u64,
    scroll: UniformListScrollHandle,
}

impl Workspace {
    /// Lists the bin again (off the UI thread), when a tab shows it.
    pub(crate) fn refresh_bin(&mut self, cx: &mut Context<Self>) {
        if !self
            .tabs
            .iter()
            .any(|tab| *tab.location() == Location::RecycleBin)
        {
            return;
        }
        // One listing at a time; changes meanwhile ask for one more.
        if self.bin.loading {
            self.bin.dirty = true;
            return;
        }
        self.bin.generation += 1;
        self.bin.loading = true;
        let generation = self.bin.generation;
        cx.spawn(async move |this, cx| {
            let listed = crate::ops::off_thread(sx_shell::recycle::list_bin).await;
            let _ = this.update(cx, |this, cx| {
                if this.bin.generation != generation {
                    return;
                }
                this.bin.loading = false;
                // The selection stays with its items, wherever they now are.
                let chosen: std::collections::HashSet<std::path::PathBuf> = this
                    .bin_chosen()
                    .iter()
                    .map(|item| item.slot.clone())
                    .collect();
                match listed {
                    Some(Ok(listing)) => {
                        let mut items = listing.items;
                        items.sort_by_key(|item| std::cmp::Reverse(item.deleted));
                        this.bin.listed = Listed::Shown;
                        this.bin.skipped = listing.skipped;
                        this.bin.items = items;
                    }
                    other => {
                        if let Some(Err(err)) = other {
                            tracing::warn!(error = %err, "recycle bin not listed");
                        }
                        this.bin.listed = Listed::Failed;
                        this.bin.items.clear();
                    }
                }
                this.bin.selected = this
                    .bin
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| chosen.contains(&item.slot))
                    .map(|(ix, _)| ix)
                    .collect();
                this.bin.anchor = None;
                if std::mem::take(&mut this.bin.dirty) {
                    this.refresh_bin(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn bin_select(&mut self, ix: usize, extend: bool, toggle: bool, cx: &mut Context<Self>) {
        let bin = &mut self.bin;
        match (extend, toggle, bin.anchor) {
            (true, _, Some(anchor)) => {
                if !toggle {
                    bin.selected.clear();
                }
                bin.selected.extend(anchor.min(ix)..=anchor.max(ix));
            }
            (_, true, _) => {
                if !bin.selected.remove(&ix) {
                    bin.selected.insert(ix);
                }
                bin.anchor = Some(ix);
            }
            _ => {
                bin.selected.clear();
                bin.selected.insert(ix);
                bin.anchor = Some(ix);
            }
        }
        cx.notify();
    }

    fn bin_chosen(&self) -> Vec<&BinItem> {
        self.bin
            .selected
            .iter()
            .filter_map(|&ix| self.bin.items.get(ix))
            .collect()
    }

    /// Puts the chosen items back where they were deleted from (renamed when the name is taken
    /// again); undoable like any operation.
    fn bin_restore(&mut self, cx: &mut Context<Self>) {
        if self.features.safety_readonly_mode {
            self.notice = Some(t!("ops.readonly").into());
            cx.notify();
            return;
        }
        let items: Vec<Binned> = self.bin_chosen().into_iter().map(binned).collect();
        if items.is_empty() {
            return;
        }
        self.run_op(
            Op::Restore {
                items,
                restored: Vec::new(),
            },
            Origin::User,
            cx,
        );
    }

    /// Deleting from the bin cannot be undone: it asks first.
    fn bin_delete(&mut self, cx: &mut Context<Self>) {
        if self.features.safety_readonly_mode {
            self.notice = Some(t!("ops.readonly").into());
            cx.notify();
            return;
        }
        let chosen = self.bin_chosen();
        let Some(first) = chosen.first() else {
            return;
        };
        let name = file_name(&first.original);
        let items = chosen.iter().map(|item| binned(item)).collect();
        self.ops.dialog = Some(Dialog::ConfirmPurge { items, name });
        cx.notify();
    }

    fn bin_empty(&mut self, cx: &mut Context<Self>) {
        if self.features.safety_readonly_mode {
            self.notice = Some(t!("ops.readonly").into());
            cx.notify();
            return;
        }
        if self.bin.items.is_empty() {
            return;
        }
        // Exactly what is listed: an item recycled since stays.
        self.ops.dialog = Some(Dialog::ConfirmEmptyBin {
            items: self.bin.items.iter().map(binned).collect(),
        });
        cx.notify();
    }

    pub(crate) fn render_recycle_bin(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let bin = &self.bin;
        let any = !bin.selected.is_empty();
        let summary: SharedString = if any {
            crate::format::selected_count(bin.selected.len())
        } else {
            crate::format::item_count(bin.items.len())
        };
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(t.px(Token::SpaceSm))
            .child(self.bin_button(
                "bin-restore",
                t!("recycle_bin.restore").into(),
                any,
                cx,
                |this, cx| {
                    this.bin_restore(cx);
                },
            ))
            .child(self.bin_button(
                "bin-delete",
                t!("recycle_bin.delete").into(),
                any,
                cx,
                |this, cx| {
                    this.bin_delete(cx);
                },
            ))
            .child(self.bin_button(
                "bin-empty",
                t!("recycle_bin.empty").into(),
                !bin.items.is_empty(),
                cx,
                Self::bin_empty,
            ))
            .child(
                div()
                    .text_size(t.px(Token::FontSizeCaption))
                    .text_color(t.color(Token::FgSecondary))
                    .child(summary),
            )
            .when(bin.skipped > 0, |el| {
                el.child(
                    div()
                        .text_size(t.px(Token::FontSizeCaption))
                        .text_color(t.color(Token::FgMuted))
                        .child(SharedString::from(t!(
                            "recycle_bin.skipped",
                            count = bin.skipped
                        ))),
                )
            });
        let body = if bin.listed == Listed::NotYet {
            self.bin_message(t!("details.loading").into(), cx)
        } else if bin.listed == Listed::Failed {
            // Honest fallback (F-LOC-06): File Explorer's own view of the bin.
            div()
                .flex()
                .flex_col()
                .gap(t.px(Token::SpaceMd))
                .child(self.bin_message(t!("recycle_bin.failed").into(), cx))
                .child(div().flex().child(self.text_button(
                    "bin-open",
                    t!("recycle_bin.open").into(),
                    cx,
                    |this, cx| this.open_shell_folder("shell:RecycleBinFolder", cx),
                )))
                .into_any_element()
        } else if bin.items.is_empty() {
            self.bin_message(t!("recycle_bin.is_empty").into(), cx)
        } else {
            self.bin_list(cx)
        };
        div()
            .id("bin-page")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(t.px(Token::SpaceLg))
            .p(t.px(Token::Space2xl))
            .child(
                div()
                    .text_size(t.px(Token::FontSizeTitle))
                    .line_height(t.px(Token::FontLineTitle))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(SharedString::from(t!("recycle_bin.title"))),
            )
            .child(toolbar)
            .child(body)
            .into_any_element()
    }

    fn bin_message(&self, text: SharedString, cx: &Context<Self>) -> AnyElement {
        div()
            .text_color(cx.tokens().color(Token::FgSecondary))
            .child(text)
            .into_any_element()
    }

    /// A toolbar button; one that cannot act now is drawn disabled and takes no click.
    fn bin_button(
        &self,
        id: &'static str,
        label: SharedString,
        enabled: bool,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
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
            .role(gpui_kit::Role::Button)
            .aria_label(label.clone())
            .when(enabled, |el| {
                el.tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
                    .hover(move |s| s.bg(hover).text_color(hover_fg).border_color(clear))
                    .active(move |s| s.bg(pressed).text_color(hover_fg).border_color(clear))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
            })
            .when(!enabled, |el| el.opacity(t.number(Token::OpacityDisabled)))
            .flex()
            .items_center()
            .h(t.px(Token::SizeControl))
            .px(t.px(Token::SpaceLg))
            .rounded(t.px(Token::RadiusSm))
            .bg(t.paint(Token::ButtonSecondaryBg))
            .border_1()
            .border_color(t.color(Token::ButtonSecondaryBorder))
            .text_color(t.color(Token::ButtonSecondaryFg))
            .child(label)
            .into_any_element()
    }

    fn bin_list(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let header = |label: SharedString| {
            div()
                .flex_1()
                .min_w_0()
                .px(t.px(Token::SpaceSm))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label)
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(t.px(Token::RowHeightDefault))
                    .px(t.px(Token::SpaceSm))
                    .text_size(t.px(Token::FontSizeCaption))
                    .text_color(t.color(Token::HeaderFg))
                    .border_b_1()
                    .border_color(t.color(Token::Divider))
                    .child(header(t!("details.name").into()))
                    .child(header(t!("recycle_bin.original").into()))
                    .child(header(t!("recycle_bin.deleted").into()))
                    .child(
                        header(t!("details.size").into())
                            .flex_none()
                            .w(t.px(Token::SizeSearchWidth) / 2.0),
                    ),
            )
            .child(
                uniform_list(
                    "bin-rows",
                    self.bin.items.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range.map(|ix| this.bin_row(ix, cx)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.bin.scroll)
                .flex_1(),
            )
            .into_any_element()
    }

    fn bin_row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let Some(item) = self.bin.items.get(ix) else {
            return div().into_any_element();
        };
        let selected = self.bin.selected.contains(&ix);
        let (selected_bg, selected_border, hover) = (
            t.color(Token::RowSelectedBg),
            t.color(Token::RowSelectedBorder),
            t.color(Token::RowHoverBg),
        );
        let name = file_name(&item.original);
        let folder = item
            .original
            .parent()
            .map(|p| shown(&p.display().to_string()))
            .unwrap_or_default();
        let deleted = sx_win::format::file_time(item.deleted).unwrap_or_default();
        let size = if item.folder {
            String::new()
        } else {
            sx_win::format::kb_size(item.size)
        };
        let cell = |text: String, color: Token| {
            div()
                .flex_1()
                .min_w_0()
                .px(t.px(Token::SpaceSm))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(t.color(color))
                .child(text)
        };
        let spoken: SharedString = format!("{name}, {folder}, {deleted}").into();
        div()
            .id(ElementId::NamedInteger("bin-row".into(), ix as u64))
            .w_full()
            .role(gpui_kit::Role::ListBoxOption)
            .aria_label(spoken)
            .aria_selected(selected)
            .flex()
            .items_center()
            .h(t.px(Token::RowHeightDefault))
            .px(t.px(Token::SpaceSm))
            .rounded(t.px(Token::RadiusSm))
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .when(selected, |el| {
                el.bg(selected_bg).border_color(selected_border)
            })
            .when(!selected, |el| el.hover(move |s| s.bg(hover)))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let m = event.modifiers();
                this.bin_select(ix, m.shift, m.control, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .gap(t.px(Token::SpaceSm))
                    .px(t.px(Token::SpaceSm))
                    .child(
                        Icon::new(if item.folder {
                            IconName::Folder
                        } else {
                            IconName::File
                        })
                        .size(t.px(Token::SizeIconSm))
                        .text_color(t.color(if item.folder {
                            Token::IconFolderFront
                        } else {
                            Token::IconDefault
                        })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name),
                    ),
            )
            .child(cell(folder, Token::FgSecondary))
            .child(cell(deleted, Token::FgSecondary))
            .child(
                cell(size, Token::FgSecondary)
                    .flex_none()
                    .w(t.px(Token::SizeSearchWidth) / 2.0),
            )
            .into_any_element()
    }
}

fn binned(item: &BinItem) -> Binned {
    Binned {
        original: item.original.clone(),
        slot: item.slot.clone(),
    }
}

/// A name from a bin record, safe to show: bidirectional controls (which can make `exe.txt`
/// read as `txt.exe`) are shown as a replacement character.
pub(crate) fn shown(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}' => '\u{FFFD}',
            c => c,
        })
        .collect()
}

fn file_name(path: &std::path::Path) -> String {
    shown(&path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn slots(items: &[BinItem]) -> Vec<PathBuf> {
        items.iter().map(|item| item.slot.clone()).collect()
    }

    #[test]
    fn bidi_controls_are_not_shown() {
        assert_eq!(shown("photo\u{202E}gpj.exe"), "photo\u{FFFD}gpj.exe");
        assert_eq!(shown("notes.txt"), "notes.txt");
    }

    #[test]
    fn names_come_from_the_original_path() {
        assert_eq!(
            file_name(std::path::Path::new(r"C:\Data\notes.txt")),
            "notes.txt"
        );
        let item = BinItem {
            slot: PathBuf::from(r"C:\$Recycle.Bin\S-1\$RAB12.txt"),
            original: PathBuf::from(r"C:\Data\notes.txt"),
            size: 1,
            deleted: 0,
            folder: false,
        };
        assert_eq!(
            slots(&[item]),
            vec![PathBuf::from(r"C:\$Recycle.Bin\S-1\$RAB12.txt")]
        );
    }
}
