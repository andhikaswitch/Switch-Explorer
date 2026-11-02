//! Tide Slash (DESIGN.md §7), the signature shape of the active sidebar item and of the active
//! tab's indicator: a highlight pill whose left side is cut at `slash.angle_deg`, with an
//! `accent.edge` line along the cut. It is painted as a path, so the cut is real; a cover-up
//! triangle would show over a translucent sidebar.

use gpui_kit::{
    Background, Bounds, Hsla, IntoElement, Path, PathBuilder, Pixels, Styled as _, canvas, point,
};

/// The slash pill filling its parent (which must be `relative`), drawn behind the content.
pub fn tide_slash(
    fill: Background,
    edge: Hsla,
    angle_deg: f32,
    radius: Pixels,
    edge_width: Pixels,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let slant = bounds.size.height * angle_deg.to_radians().tan();
            if let Some(path) = pill(bounds, slant, radius) {
                window.paint_path(path, fill);
            }
            let half = edge_width / 2.0;
            let mut line = PathBuilder::stroke(edge_width);
            line.move_to(point(bounds.left() + slant + half, bounds.top() + half));
            line.line_to(point(bounds.left() + half, bounds.bottom() - half));
            if let Ok(path) = line.build() {
                window.paint_path(path, edge);
            }
        },
    )
    .absolute()
    .inset_0()
}

/// The active tab's underline: a parallelogram slanted like the slash.
pub fn slanted_bar(fill: Background, angle_deg: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let slant = bounds.size.height * angle_deg.to_radians().tan();
            let mut bar = PathBuilder::fill();
            bar.move_to(point(bounds.left() + slant, bounds.top()));
            bar.line_to(point(bounds.right(), bounds.top()));
            bar.line_to(point(bounds.right() - slant, bounds.bottom()));
            bar.line_to(point(bounds.left(), bounds.bottom()));
            bar.close();
            if let Ok(path) = bar.build() {
                window.paint_path(path, fill);
            }
        },
    )
    .size_full()
}

/// The pill outline: cut left edge, rounded right corners.
fn pill(b: Bounds<Pixels>, slant: Pixels, radius: Pixels) -> Option<Path<Pixels>> {
    let r = if radius * 2.0 > b.size.height {
        b.size.height / 2.0
    } else {
        radius
    };
    let (left, top, right, bottom) = (b.left(), b.top(), b.right(), b.bottom());
    let mut p = PathBuilder::fill();
    p.move_to(point(left + slant, top));
    p.line_to(point(right - r, top));
    p.curve_to(point(right, top + r), point(right, top));
    p.line_to(point(right, bottom - r));
    p.curve_to(point(right - r, bottom), point(right, bottom));
    p.line_to(point(left, bottom));
    p.close();
    p.build().ok()
}
