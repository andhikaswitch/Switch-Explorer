//! Overlays for file operations: confirmation dialogs, the conflict sheet (F-OPS-05), progress
//! cards, the result toast with Undo, and the Transfer panel (F-OPS-06, `Ctrl+J`).

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    relative,
};
use rust_i18n::t;
use sx_ops::Resolution;
use sx_theme::Token;

use crate::ops::{Dialog, Job};
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// `SystemTime` in the Windows short date/time format.
fn when(time: Option<SystemTime>) -> String {
    const EPOCH_AS_FILETIME: i64 = 116_444_736_000_000_000;
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos() / 100).ok())
        .and_then(|ticks| sx_win::format::file_time(ticks + EPOCH_AS_FILETIME))
        .unwrap_or_default()
}

impl Workspace {
    /// Everything drawn above the window content, in order.
    pub(crate) fn render_overlays(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if self.ops.transfer_panel {
            out.push(self.render_transfer_panel(cx));
        }
        out.push(self.render_corner(cx));
        if self.ops.dialog.is_some() {
            out.push(self.render_dialog(window, cx));
        }
        out
    }

    // ---- Dialogs --------------------------------------------------------------------------

    fn render_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.dialog_focus.contains_focused(window, cx) {
            let focus = self.dialog_focus.clone();
            window.defer(cx, move |window, cx| window.focus(&focus, cx));
        }
        let t = cx.tokens();
        let body = match &self.ops.dialog {
            Some(Dialog::ConfirmRecycle(paths)) => self.confirm_body(
                if paths.len() == 1 {
                    t!("ops.confirm_recycle_one", name = file_name(&paths[0]))
                } else {
                    t!("ops.confirm_recycle_many", count = paths.len())
                }
                .into(),
                t!("ops.recycle").into(),
                false,
                cx,
            ),
            Some(Dialog::ConfirmDelete(paths)) => self.confirm_body(
                if paths.len() == 1 {
                    t!("ops.confirm_delete_one", name = file_name(&paths[0]))
                } else {
                    t!("ops.confirm_delete_many", count = paths.len())
                }
                .into(),
                t!("ops.delete_permanently").into(),
                true,
                cx,
            ),
            Some(Dialog::Conflict(_)) => self.conflict_body(cx),
            None => div().into_any_element(),
        };
        div()
            .id("dialog-scrim")
            .key_context("Dialog")
            .track_focus(&self.dialog_focus)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(t.paint(Token::Scrim))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.dismiss_dialog(cx),
                    "enter" if !matches!(this.ops.dialog, Some(Dialog::Conflict(_))) => {
                        this.confirm_dialog(cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id("dialog")
                    .occlude()
                    .flex()
                    .flex_col()
                    .gap(t.px(Token::SpaceLg))
                    .w(t.px(Token::SizeSidebar) * 2.0)
                    .p(t.px(Token::Space2xl))
                    .rounded(t.px(Token::RadiusXl))
                    .bg(t.paint(Token::SurfaceOverlay))
                    .shadow(t.shadow(Token::ShadowDialog))
                    .child(body),
            )
            .into_any_element()
    }

    fn confirm_body(
        &self,
        message: SharedString,
        action: SharedString,
        danger: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.tokens();
        let (bg, hover, fg) = if danger {
            (
                t.paint(Token::ButtonDangerBg),
                t.paint(Token::ButtonDangerBgHover),
                t.color(Token::ButtonDangerFg),
            )
        } else {
            (
                t.paint(Token::ButtonPrimaryBg),
                t.paint(Token::ButtonPrimaryBgHover),
                t.color(Token::ButtonPrimaryFg),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap(t.px(Token::SpaceXl))
            .child(div().text_color(t.color(Token::FgPrimary)).child(message))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(t.px(Token::SpaceMd))
                    .child(self.text_button(
                        "dialog-cancel",
                        t!("ops.cancel").into(),
                        cx,
                        |this, cx| {
                            this.dismiss_dialog(cx);
                        },
                    ))
                    .child(
                        div()
                            .id("dialog-confirm")
                            .flex()
                            .items_center()
                            .h(t.px(Token::SizeControl))
                            .px(t.px(Token::SpaceLg))
                            .rounded(t.px(Token::RadiusSm))
                            .bg(bg)
                            .text_color(fg)
                            .hover(move |s| s.bg(hover))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.confirm_dialog(cx)),
                            )
                            .child(action),
                    ),
            )
            .into_any_element()
    }

    fn conflict_body(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let Some(Dialog::Conflict(pending)) = &self.ops.dialog else {
            return div().into_any_element();
        };
        let Some(conflict) = pending.conflicts.get(pending.index) else {
            return div().into_any_element();
        };
        let remaining = pending.conflicts.len() - pending.index;
        let caption = |text: SharedString| {
            div()
                .text_size(t.px(Token::FontSizeCaption))
                .line_height(t.px(Token::FontLineCaption))
                .text_color(t.color(Token::FgSecondary))
                .child(text)
        };
        let choice = |id: &'static str, label: SharedString, resolution: Resolution| {
            self.text_button(id, label, cx, move |this, cx| {
                this.decide_conflict(resolution, cx);
            })
        };
        div()
            .flex()
            .flex_col()
            .gap(t.px(Token::SpaceLg))
            .child(
                div()
                    .text_size(t.px(Token::FontSizeSubtitle))
                    .line_height(t.px(Token::FontLineSubtitle))
                    .child(SharedString::from(t!(
                        "conflict.title",
                        name = file_name(&conflict.target)
                    ))),
            )
            .child(caption(
                t!(
                    "conflict.incoming",
                    size = sx_win::format::kb_size(conflict.source_size),
                    date = when(conflict.source_modified)
                )
                .into(),
            ))
            .child(caption(
                t!(
                    "conflict.existing",
                    size = sx_win::format::kb_size(conflict.target_size),
                    date = when(conflict.target_modified)
                )
                .into(),
            ))
            .when(remaining > 1, |el| {
                el.child(self.apply_all_toggle(pending.apply_all, remaining, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(t.px(Token::SpaceMd))
                    .child(choice(
                        "conflict-skip",
                        t!("conflict.skip").into(),
                        Resolution::Skip,
                    ))
                    .child(choice(
                        "conflict-newer",
                        t!("conflict.replace_newer").into(),
                        Resolution::ReplaceIfNewer,
                    ))
                    .child(choice(
                        "conflict-both",
                        t!("conflict.keep_both").into(),
                        Resolution::KeepBoth,
                    ))
                    .child(choice(
                        "conflict-replace",
                        t!("conflict.replace").into(),
                        Resolution::Replace,
                    )),
            )
            .into_any_element()
    }

    /// "Do this for all N items" under the conflict sheet.
    fn apply_all_toggle(&self, checked: bool, remaining: usize, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        div()
            .id("conflict-all")
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceSm))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                if let Some(Dialog::Conflict(pending)) = this.ops.dialog.as_mut() {
                    pending.apply_all = !pending.apply_all;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(t.px(Token::SizeIconSm))
                    .rounded(t.px(Token::RadiusXs))
                    .border_1()
                    .border_color(t.color(Token::CheckboxBorder))
                    .when(checked, |b| {
                        b.bg(t.paint(Token::CheckboxCheckedBg))
                            .text_color(t.color(Token::CheckboxCheck))
                            .child(Icon::new(IconName::Check).size(t.px(Token::SizeIconSm)))
                    }),
            )
            .child(SharedString::from(t!(
                "conflict.apply_all",
                count = remaining
            )))
            .into_any_element()
    }

    // ---- Progress, toast ------------------------------------------------------------------

    fn job_card(&self, job: &Job, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let fraction = if job.progress.total == 0 {
            0.0
        } else {
            job.progress.done as f32 / job.progress.total as f32
        };
        let cancel = job.cancel.clone();
        div()
            .flex()
            .flex_col()
            .gap(t.px(Token::SpaceXs))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(t.px(Token::SpaceMd))
                    .child(div().flex_1().min_w_0().child(job.label.clone()))
                    .child(SharedString::from(format!(
                        "{} / {}",
                        job.progress.done.min(job.progress.total),
                        job.progress.total
                    )))
                    .child(
                        div()
                            .id(ElementId::NamedInteger("job-cancel".into(), job.id))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(t.px(Token::SizeControl))
                            .rounded(t.px(Token::RadiusSm))
                            .hover({
                                let hover = t.paint(Token::ButtonGhostBgHover);
                                let fg = t.color(Token::ButtonGhostFgHover);
                                move |s| s.bg(hover).text_color(fg)
                            })
                            .on_click(move |_, _, _| cancel.cancel())
                            .child(Icon::new(IconName::Close).size(t.px(Token::SizeIconSm))),
                    ),
            )
            .child(
                div()
                    .h(t.px(Token::SpaceXs))
                    .w_full()
                    .rounded(t.px(Token::RadiusXs))
                    .bg(t.color(Token::ProgressTrack))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded(t.px(Token::RadiusXs))
                            .bg(t.paint(Token::ProgressFill)),
                    ),
            )
            .children(job.progress.current.as_ref().map(|p| {
                div()
                    .text_size(t.px(Token::FontSizeCaption))
                    .line_height(t.px(Token::FontLineCaption))
                    .text_color(t.color(Token::FgSecondary))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(file_name(p)))
            }))
            .into_any_element()
    }

    /// Bottom-right corner: running jobs, then the last result with Undo.
    fn render_corner(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let card = || {
            div()
                .occlude()
                .flex()
                .flex_col()
                .gap(t.px(Token::SpaceSm))
                .w(t.px(Token::SizeSidebar) * 1.5)
                .p(t.px(Token::SpaceLg))
                .rounded(t.px(Token::RadiusLg))
                .bg(t.paint(Token::ToastBg))
                .text_color(t.color(Token::ToastFg))
                .shadow(t.shadow(Token::ShadowPopover))
        };
        let running: Vec<AnyElement> = if self.ops.transfer_panel {
            Vec::new()
        } else {
            self.ops
                .running()
                .map(|job| self.job_card(job, cx))
                .collect()
        };
        div()
            .absolute()
            .right(t.px(Token::SpaceXl))
            .bottom(t.px(Token::SizeStatusbar) + t.px(Token::SpaceLg))
            .flex()
            .flex_col()
            .items_end()
            .gap(t.px(Token::SpaceMd))
            .children(running.into_iter().map(|job| card().child(job)))
            .children(self.ops.toast.as_ref().map(|toast| {
                card().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(t.px(Token::SpaceMd))
                        .child(div().flex_1().min_w_0().child(toast.text.clone()))
                        .when(toast.undo, |el| {
                            el.child(self.text_button(
                                "toast-undo",
                                t!("ops.undo").into(),
                                cx,
                                |this, cx| {
                                    this.undo(cx);
                                },
                            ))
                        }),
                )
            }))
            .into_any_element()
    }

    // ---- Transfer panel -------------------------------------------------------------------

    fn render_transfer_panel(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let section = |label: SharedString| {
            div()
                .pt(t.px(Token::SpaceMd))
                .text_size(t.px(Token::FontSizeCaption))
                .line_height(t.px(Token::FontLineCaption))
                .text_color(t.color(Token::FgMuted))
                .child(label)
        };
        let finished: Vec<&Job> = self
            .ops
            .jobs
            .iter()
            .filter(|j| j.finished.is_some())
            .rev()
            .collect();
        let empty = self.ops.jobs.is_empty() && self.ops.interrupted.is_empty();
        div()
            .id("transfer-panel")
            .occlude()
            .absolute()
            .top(t.px(Token::SizeTitlebar) + t.px(Token::SizeCommandbar))
            .right_0()
            .bottom(t.px(Token::SizeStatusbar))
            .w(t.px(Token::SizeSidebar) * 1.5)
            .flex()
            .flex_col()
            .gap(t.px(Token::SpaceMd))
            .p(t.px(Token::SpaceLg))
            .bg(t.paint(Token::SurfaceOverlay))
            .shadow(t.shadow(Token::ShadowPopover))
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(t.px(Token::FontSizeSubtitle))
                    .line_height(t.px(Token::FontLineSubtitle))
                    .child(SharedString::from(t!("ops.transfer_title"))),
            )
            .when(empty, |el| {
                el.child(
                    div()
                        .text_color(t.color(Token::FgMuted))
                        .child(SharedString::from(t!("ops.transfer_empty"))),
                )
            })
            .children(self.ops.running().map(|job| self.job_card(job, cx)))
            .when(!finished.is_empty(), |el| {
                el.child(section(t!("ops.transfer_done").into()))
            })
            .children(finished.into_iter().map(|job| {
                div().flex().flex_col().child(job.label.clone()).child(
                    div()
                        .text_size(t.px(Token::FontSizeCaption))
                        .line_height(t.px(Token::FontLineCaption))
                        .text_color(t.color(Token::FgSecondary))
                        .children(job.finished.clone()),
                )
            }))
            .when(!self.ops.interrupted.is_empty(), |el| {
                el.child(section(t!("ops.interrupted_title").into()))
                    .children(
                        self.ops
                            .interrupted
                            .iter()
                            .enumerate()
                            .map(|(ix, (_, op))| {
                                let op = op.clone();
                                div()
                                    .id(ElementId::NamedInteger("interrupted".into(), ix as u64))
                                    .flex()
                                    .items_center()
                                    .gap(t.px(Token::SpaceSm))
                                    .child(div().flex_1().child(SharedString::from(t!(
                                        "ops.interrupted_item",
                                        count = op.item_count()
                                    ))))
                                    .child(self.text_button(
                                        "interrupted-open",
                                        t!("ops.open_folder").into(),
                                        cx,
                                        move |this, cx| this.open_location_of(&op, cx),
                                    ))
                            }),
                    )
                    .child(self.text_button(
                        "interrupted-dismiss",
                        t!("ops.dismiss").into(),
                        cx,
                        Workspace::dismiss_interrupted,
                    ))
            })
            .into_any_element()
    }
}
