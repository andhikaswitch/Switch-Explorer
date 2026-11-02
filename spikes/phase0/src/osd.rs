//! On-screen display for switch changes (ADR-014). In-app changes draw an overlay inside the
//! workspace; a global hotkey fired while the app is in the background opens a small topmost
//! window instead. The spike uses that window to measure what an extra GPUI window costs
//! (time to first frame, private working set).

use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use crate::{bench, win};

pub struct OsdView {
    title: SharedString,
    detail: SharedString,
}

impl Render for OsdView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .px_4()
            .gap_1()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(self.detail.clone()),
            )
    }
}

pub fn open_global(
    title: SharedString,
    detail: SharedString,
    requested: Instant,
    cx: &mut App,
) -> anyhow::Result<()> {
    let display = cx
        .primary_display()
        .ok_or_else(|| anyhow::anyhow!("no display"))?;
    let area = display.bounds();
    let size = size(px(400.), px(72.));
    let origin = point(
        area.origin.x + (area.size.width - size.width) / 2.,
        area.origin.y + area.size.height - size.height - px(120.),
    );
    let before = win::private_working_set();
    let options = WindowOptions {
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size })),
        ..Default::default()
    };
    let handle = cx.open_window(options, move |window, cx| {
        window.on_next_frame(move |_, _| {
            log::info!(
                target: "sx_spike",
                "osd-window: first frame {:.1} ms after hotkey",
                bench::ms(requested.elapsed())
            );
        });
        cx.new(|_| OsdView { title, detail })
    })?;
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(700))
            .await;
        if let (Some(before), Some(after)) = (before, win::private_working_set()) {
            log::info!(
                target: "sx_spike",
                "osd-window: private working set {:+.1} MB with the extra window",
                (after as f64 - before as f64) / 1_048_576.0
            );
        }
        cx.background_executor()
            .timer(Duration::from_millis(1000))
            .await;
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    })
    .detach();
    Ok(())
}
