//! The Home page (FEATURES.md F-HOME, DESIGN.md §8.2): storage gauges, pinned folders, what the
//! last operations did (with Undo), and the files used last. Recent files come from Windows'
//! own Recent Items (`sx_shell::recent`), read off the UI thread and only while
//! `home.show_recent` is on; nothing is tracked here.
//!
//! A recent file can be on a share that is gone, so nothing here asks the file itself for
//! anything before the user opens it: its row shows the icon of its type.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::SystemTime;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, div, relative,
};
use rust_i18n::t;
use sx_core::Location;
use sx_shell::recent::RecentFile;
use sx_theme::Token;

use crate::focus::TabStop as _;
use crate::format;
use crate::ops::Origin;
use crate::theme::ActiveTokens as _;
use crate::workspace::{LOADING_DELAY, Workspace};

/// Recent files listed.
const RECENT_FILES: usize = 10;
/// Operations listed under "Recent activity" (F-HOME-05).
const ACTIVITY: usize = 5;

/// The page's direct children, by index: what the keyboard's focus scrolls into view.
const CHILD_STORAGE: usize = 1;
const CHILD_PINS_AND_ACTIVITY: usize = 2;
/// The recent rows follow their heading, one child each.
const CHILD_FIRST_RECENT: usize = 4;

/// Where the last reading of the recent files got to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Listed {
    #[default]
    NotYet,
    Shown,
    Failed,
}

/// A recent file, with its texts made once per listing (not per frame).
#[derive(Debug)]
struct RecentRow {
    path: PathBuf,
    name: SharedString,
    folder: SharedString,
    when: SharedString,
    spoken: SharedString,
}

impl RecentRow {
    fn of(file: RecentFile, now: SystemTime) -> Self {
        let shown = crate::bin_page::shown;
        let name = file.path.file_name().map_or_else(
            || shown(&file.path.display().to_string()),
            |name| shown(&name.to_string_lossy()),
        );
        let folder = file
            .path
            .parent()
            .map(|parent| shown(&parent.display().to_string()))
            .unwrap_or_default();
        let when = format::ago(file.used, now);
        let spoken = t!("home.recent_row", name = name, folder = folder, when = when);
        Self {
            path: file.path,
            name: name.into(),
            folder: folder.into(),
            when,
            spoken: spoken.into(),
        }
    }
}

/// What Home shows beyond the workspace's own state (drives, pins, the undo history).
#[derive(Debug, Default)]
pub(crate) struct HomeView {
    recent: Vec<RecentRow>,
    listed: Listed,
    loading: bool,
    /// The first reading takes long enough to say so (DESIGN.md §11: nothing before 150 ms).
    slow: bool,
    scroll: ScrollHandle,
    /// The page child holding the focused stop, found while drawing.
    focus_child: Cell<Option<usize>>,
    /// The child last scrolled into view for the focus: scrolled once per focus move, so the
    /// wheel stays free afterwards.
    scrolled_for: Cell<Option<usize>>,
}

impl Workspace {
    /// Brings Home up to date, when a tab shows it: the drives' free space and the recent
    /// files (off the UI thread). With `home.show_recent` off, the list and its memory go and
    /// the Recent Items folder is not read.
    pub(crate) fn refresh_home(&mut self, cx: &mut Context<Self>) {
        if !self.features.home_show_recent {
            self.home.recent = Vec::new();
            self.home.listed = Listed::NotYet;
            self.home.slow = false;
            cx.notify();
        }
        if !self
            .tabs
            .iter()
            .any(|tab| *tab.location() == Location::Home)
        {
            return;
        }
        self.refresh_drives(cx);
        // One reading at a time.
        if !self.features.home_show_recent || std::mem::replace(&mut self.home.loading, true) {
            return;
        }
        if self.home.listed == Listed::NotYet {
            let delay = cx.background_executor().timer(LOADING_DELAY);
            cx.spawn(async move |this, cx| {
                delay.await;
                let _ = this.update(cx, |this, cx| {
                    if this.home.loading && this.home.listed == Listed::NotYet {
                        this.home.slow = true;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            let listed = crate::ops::off_thread(|| {
                sx_shell::recent::recent_files(RECENT_FILES).map(|files| {
                    let now = SystemTime::now();
                    files
                        .into_iter()
                        .map(|file| RecentRow::of(file, now))
                        .collect::<Vec<_>>()
                })
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                this.home.loading = false;
                this.home.slow = false;
                // Switched off while it was read: it is not shown, and not kept.
                if !this.features.home_show_recent {
                    return;
                }
                match listed {
                    Some(Ok(rows)) => {
                        // Their type icons are asked for now (again after the cache dropped
                        // them); drawing only looks them up.
                        for row in &rows {
                            let _ = this.icons.get_type(&row.path);
                        }
                        this.home.recent = rows;
                        this.home.listed = Listed::Shown;
                    }
                    other => {
                        if let Some(Err(err)) = other {
                            tracing::warn!(error = %err, "recent files not listed");
                        }
                        this.home.recent.clear();
                        this.home.listed = Listed::Failed;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens a recent file with its default app; a failure says why.
    fn open_recent(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.home.recent.get(ix) else {
            return;
        };
        let name = row.name.to_string();
        let open = sx_shell::exec::open(row.path.clone(), self.hwnd);
        cx.spawn(async move |this, cx| {
            let opened = open.await;
            let _ = this.update(cx, |this, cx| match opened {
                // It is the most recent file now.
                Ok(()) => this.refresh_home(cx),
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    tracing::debug!(error = %err, "open failed");
                    this.notice = Some(format::open_error(&name, &err));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Notes that the stop `focus` (in page child `child`) has the keyboard's focus.
    fn home_stop(&self, focus: &FocusHandle, child: usize) {
        if self.focused_now.as_ref() == Some(focus) {
            self.home.focus_child.set(Some(child));
        }
    }

    pub(crate) fn render_home(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        let column = t.px(Token::SizeSidebar);
        let section_gap = t.px(Token::Space2xl);
        let mut page = div()
            .id("page")
            .flex()
            .flex_col()
            .flex_1()
            .p(t.px(Token::Space2xl))
            .overflow_y_scroll()
            .track_scroll(&self.home.scroll)
            .child(self.page_title(t!("home.title").into(), cx))
            .child(
                self.home_section("home-storage", t!("home.storage").into(), cx)
                    .mt(t.px(Token::SpaceLg))
                    .child(self.home_storage(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    // Side by side, or one under the other in a narrow window.
                    .gap(section_gap)
                    .mt(section_gap)
                    .child(
                        self.home_section("home-pinned", t!("nav.pinned").into(), cx)
                            .flex_1()
                            .min_w(column)
                            .child(self.home_pins(cx)),
                    )
                    .child(
                        self.home_section("home-activity", t!("home.activity").into(), cx)
                            .flex_1()
                            .min_w(column)
                            .child(self.home_activity(cx)),
                    ),
            );
        if self.features.home_show_recent {
            page = page
                .child(
                    self.home_section("home-recent", t!("home.recent").into(), cx)
                        .mt(section_gap),
                )
                .children(self.home_recent(cx));
        }
        // The keyboard's focus is never left below the fold: its part of the page is scrolled
        // into view when it moves there.
        let focused = self.home.focus_child.take();
        if focused != self.home.scrolled_for.replace(focused)
            && let Some(child) = focused
        {
            self.home.scroll.scroll_to_item(child);
        }
        page
    }

    /// A section: its heading, with the body added by the caller.
    fn home_section(&self, id: &'static str, title: SharedString, cx: &Context<Self>) -> Div {
        let t = cx.tokens();
        div().flex().flex_col().gap(t.px(Token::SpaceSm)).child(
            div()
                .id(id)
                .role(gpui_kit::Role::Heading)
                .aria_level(2)
                .text_size(t.px(Token::FontSizeSubtitle))
                .line_height(t.px(Token::FontLineSubtitle))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title),
        )
    }

    /// A line of quiet text: an empty section, or one still loading.
    fn home_note(&self, text: SharedString, cx: &Context<Self>) -> AnyElement {
        div()
            .text_color(cx.tokens().color(Token::FgSecondary))
            .child(text)
            .into_any_element()
    }

    /// One compact gauge row per drive (F-HOME-01); a click opens the drive.
    fn home_storage(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let (hover, pressed) = (t.color(Token::RowHoverBg), t.color(Token::PressedBg));
        div()
            .flex()
            .flex_wrap()
            .gap_x(t.px(Token::SpaceXl))
            .gap_y(t.px(Token::SpaceXs))
            .children(self.drives.iter().map(|drive| {
                let id = ElementId::NamedInteger("home-drive".into(), u64::from(drive.letter));
                let focus = self.focus_stop(&id, cx);
                self.home_stop(&focus, CHILD_STORAGE);
                let label = format::drive_label(drive);
                let space = format::drive_used(drive);
                let used = drive.space.map(sx_win::Space::used_fraction);
                let root = drive.root.clone();
                div()
                    .id(id)
                    .role(gpui_kit::Role::Button)
                    .aria_label(SharedString::from(format!("{label}, {space}")))
                    .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
                    .flex()
                    .items_center()
                    .gap(t.px(Token::SpaceMd))
                    .w(t.px(Token::SizeHomeStorageRow))
                    .max_w_full()
                    .h(t.px(Token::RowHeightDefault))
                    .px(t.px(Token::SpaceSm))
                    .rounded(t.px(Token::RadiusSm))
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(pressed))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(Location::Path(root.clone()), cx);
                    }))
                    .child(
                        Icon::new(IconName::HardDrive)
                            .size(t.px(Token::SizeIconSm))
                            .flex_none()
                            .text_color(t.color(Token::NavItemIcon)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(t.px(Token::SizeHomeLabel))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(t.px(Token::Space3xl))
                            .h(t.px(Token::SpaceXs))
                            .rounded(t.px(Token::RadiusXs))
                            .bg(t.color(Token::GaugeTrack))
                            .when_some(used, |track, used| {
                                track.child(
                                    div()
                                        .h_full()
                                        .w(relative(used))
                                        .rounded(t.px(Token::RadiusXs))
                                        .bg(t.paint(crate::sidebar::gauge_fill(used))),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(t.px(Token::FontSizeCaption))
                            .line_height(t.px(Token::FontLineCaption))
                            .text_color(t.color(Token::FgSecondary))
                            .child(space),
                    )
            }))
            .into_any_element()
    }

    /// The pinned folders as cards (F-HOME-02); a click opens the folder.
    fn home_pins(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        if self.pins.is_empty() {
            return self.home_note(t!("home.no_pins").into(), cx);
        }
        let (hover, hover_fg) = (
            t.paint(Token::SurfaceCardHover),
            t.color(Token::CardHoverFg),
        );
        div()
            .flex()
            .flex_wrap()
            .gap(t.px(Token::SpaceSm))
            .children(self.pins.iter().enumerate().map(|(ix, pin)| {
                let id = ElementId::NamedInteger("home-pin".into(), ix as u64);
                let lit = self.hovered.as_ref() == Some(&id);
                let focus = self.focus_stop(&id, cx);
                self.home_stop(&focus, CHILD_PINS_AND_ACTIVITY);
                let to = pin.path.clone();
                let hovered_id = id.clone();
                div()
                    .id(id)
                    .role(gpui_kit::Role::Button)
                    .aria_label(pin.name.clone())
                    .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusLg)), cx)
                    .flex()
                    .items_center()
                    .gap(t.px(Token::SpaceSm))
                    .h(t.px(Token::SizeControl))
                    .max_w(t.px(Token::SizeSidebar))
                    .px(t.px(Token::SpaceLg))
                    .rounded(t.px(Token::RadiusLg))
                    .bg(t.paint(Token::SurfaceCard))
                    .shadow(t.shadow(Token::ShadowCard))
                    .when(lit, |el| el.bg(hover).text_color(hover_fg))
                    .hover(move |s| s.bg(hover).text_color(hover_fg))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.set_hovered(&hovered_id, *hovered, cx);
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(Location::Path(to.clone()), cx);
                    }))
                    .child(
                        Icon::new(IconName::Folder)
                            .size(t.px(Token::SizeIconSm))
                            .flex_none()
                            .text_color(if lit {
                                hover_fg
                            } else {
                                t.color(Token::IconFolderFront)
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(pin.name.clone()),
                    )
            }))
            .into_any_element()
    }

    /// What the last operations did, newest first (F-HOME-05). Undo takes operations back in
    /// that order, so only the newest one has the button.
    fn home_activity(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let mut done = self.ops.undo.recent(ACTIVITY).peekable();
        if done.peek().is_none() {
            return self.home_note(t!("home.no_activity").into(), cx);
        }
        let readonly = self.features.safety_readonly_mode;
        div()
            .flex()
            .flex_col()
            .children(done.enumerate().map(|(ix, op)| {
                let text = crate::ops::done_text(op, Origin::User);
                div()
                    .flex()
                    .items_center()
                    .gap(t.px(Token::SpaceMd))
                    .min_h(t.px(Token::SizeControl))
                    .py(t.px(Token::SpaceXxs))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .when(ix > 0, |el| el.text_color(t.color(Token::FgSecondary)))
                            .child(text.clone()),
                    )
                    .when(ix == 0, |el| {
                        let focus = self.focus_stop(&ElementId::Name("home-undo".into()), cx);
                        self.home_stop(&focus, CHILD_PINS_AND_ACTIVITY);
                        // Read-only mode refuses every undo: the button says so up front. A
                        // screen reader hears what the button takes back.
                        el.child(self.action_button(
                            "home-undo",
                            t!("ops.undo").into(),
                            !readonly,
                            if readonly {
                                t!("ops.readonly").into()
                            } else {
                                text
                            },
                            cx,
                            Self::undo,
                        ))
                    })
            }))
            .into_any_element()
    }

    /// The files used last (F-HOME-04), one page child per row: a click opens the file, the
    /// folder button its place.
    fn home_recent(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let home = &self.home;
        let t = cx.tokens();
        let note = |text: SharedString| {
            vec![
                div()
                    .mt(t.px(Token::SpaceSm))
                    .child(self.home_note(text, cx))
                    .into_any_element(),
            ]
        };
        match home.listed {
            // Nothing for a moment, then "Loading…" (DESIGN.md §11).
            Listed::NotYet if home.slow => note(t!("details.loading").into()),
            Listed::NotYet => Vec::new(),
            Listed::Failed => note(t!("home.recent_failed").into()),
            Listed::Shown if home.recent.is_empty() => note(t!("home.no_recent").into()),
            Listed::Shown => home
                .recent
                .iter()
                .enumerate()
                .map(|(ix, row)| self.home_recent_row(ix, row, cx))
                .collect(),
        }
    }

    fn home_recent_row(&self, ix: usize, row: &RecentRow, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let (hover, pressed) = (t.color(Token::RowHoverBg), t.color(Token::PressedBg));
        let id = ElementId::NamedInteger("home-recent".into(), ix as u64);
        let focus = self.focus_stop(&id, cx);
        self.home_stop(&focus, CHILD_FIRST_RECENT + ix);
        let cell = |text: SharedString| {
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text)
        };
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(row.spoken.clone())
            .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
            .flex()
            .flex_none()
            .items_center()
            .gap(t.px(Token::SpaceMd))
            .h(t.px(Token::RowHeightDefault))
            .px(t.px(Token::SpaceSm))
            // The first row keeps the section's gap under its heading.
            .when(ix == 0, |el| el.mt(t.px(Token::SpaceSm)))
            .rounded(t.px(Token::RadiusSm))
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_recent(ix, cx);
            }))
            // The icon of its type: the file itself is not asked (it may be on a dead share).
            .child(match self.icons.peek_type(&row.path) {
                Some(image) => gpui_kit::img(image)
                    .size(t.px(Token::SizeIconSm))
                    .flex_none()
                    .into_any_element(),
                None => Icon::new(IconName::File)
                    .size(t.px(Token::SizeIconSm))
                    .flex_none()
                    .text_color(t.color(Token::IconDefault))
                    .into_any_element(),
            })
            .child(cell(row.name.clone()).flex_1())
            .child(
                cell(row.folder.clone())
                    .flex_1()
                    .text_color(t.color(Token::FgSecondary)),
            )
            .child(
                cell(row.when.clone())
                    .flex_none()
                    .w(t.px(Token::SizeHomeLabel))
                    .text_color(t.color(Token::FgSecondary)),
            )
            .child(self.home_location_button(ix, row, cx))
            .into_any_element()
    }

    /// Opens the folder a recent file is in, with the file selected.
    fn home_location_button(&self, ix: usize, row: &RecentRow, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let (hover, pressed, hover_fg) = (
            t.paint(Token::ButtonGhostBgHover),
            t.paint(Token::ButtonGhostBgPressed),
            t.color(Token::ButtonGhostFgHover),
        );
        let id = ElementId::NamedInteger("home-recent-place".into(), ix as u64);
        let focus = self.focus_stop(&id, cx);
        self.home_stop(&focus, CHILD_FIRST_RECENT + ix);
        let label: SharedString = t!("home.open_location").into();
        let tip = label.clone();
        let place = row.path.parent().map(std::path::Path::to_path_buf).zip(
            row.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
        );
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            // Ten rows have this button: which file it is for.
            .aria_description(row.name.clone())
            .tab_stop_ring(&focus, self.ring_for(&focus, t.px(Token::RadiusSm)), cx)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(t.px(Token::SizeControl))
            .rounded(t.px(Token::RadiusSm))
            .text_color(t.color(Token::ButtonGhostFg))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .hover(move |s| s.bg(hover).text_color(hover_fg))
            .active(move |s| s.bg(pressed).text_color(hover_fg))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                // The row under the button opens the file; this click is only the folder's.
                cx.stop_propagation();
                if let Some((folder, file)) = place.clone() {
                    this.navigate(Location::Path(folder), cx);
                    this.tab_mut().reveal = Some(file);
                }
            }))
            .child(Icon::new(IconName::FolderOpen).size(t.px(Token::SizeIconSm)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_recent_row_shows_name_folder_and_age() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(500_000);
        let row = RecentRow::of(
            RecentFile {
                path: PathBuf::from("D:\\Received\\report\u{202E}fdp.exe"),
                used: now - Duration::from_hours(2),
            },
            now,
        );
        // Bidi controls in a name are never shown as they are.
        assert_eq!(row.name.as_ref(), "report\u{FFFD}fdp.exe");
        assert_eq!(row.folder.as_ref(), r"D:\Received");
        assert_eq!(row.when.as_ref(), "2 hours ago");
        assert!(row.spoken.contains("2 hours ago") && row.spoken.contains(r"D:\Received"));
    }
}
