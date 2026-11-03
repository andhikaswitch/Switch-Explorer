//! Tide Slash (DESIGN.md §7), the signature shape of the active sidebar item: a highlight pill
//! whose left side is cut at `slash.angle_deg`, with an `accent.edge` line along the cut.
//!
//! It is rasterised on the CPU (analytic anti-aliasing, the theme's gradient) and drawn as an
//! image, cached per pixel size and colours. GPUI's path pipeline is avoided on purpose: the
//! first path drawn commits a window-sized 4× MSAA texture, about 29 MB on an iGPU (Phase 1
//! benchmark), for one small shape.

// Geometry and colour maths read best with the usual short names (x, y, w, h, r, t).
#![allow(clippy::many_single_char_names)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{Corners, IntoElement, RenderImage, Styled as _, canvas};
use sx_theme::{Paint, Rgba};

/// Cached shapes; a handful of sizes exist at a time (one per active item and scale).
const CACHE_LIMIT: usize = 32;

thread_local! {
    static CACHE: RefCell<HashMap<Key, Arc<RenderImage>>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    width: u32,
    height: u32,
    /// Bit patterns of every float input, so equal inputs hash equally.
    params: Vec<u32>,
}

/// Everything that shapes the pill, in physical pixels.
#[derive(Clone, Debug)]
pub struct Slash {
    pub fill: Paint,
    pub edge: Rgba,
    pub angle_deg: f32,
    pub radius: f32,
    pub edge_width: f32,
}

impl Slash {
    fn key(&self, width: u32, height: u32) -> Key {
        let mut params: Vec<u32> = vec![
            self.angle_deg.to_bits(),
            self.radius.to_bits(),
            self.edge_width.to_bits(),
        ];
        let bits = |c: &Rgba| [c.r, c.g, c.b, c.a].map(f32::to_bits);
        params.extend(bits(&self.edge));
        match &self.fill {
            Paint::Solid(c) => params.extend(bits(c)),
            Paint::Linear { angle, stops } => {
                params.push(angle.to_bits());
                params.extend(stops.iter().flat_map(bits));
            }
        }
        Key {
            width,
            height,
            params,
        }
    }
}

/// The slash pill filling its parent (which must be `relative`), drawn behind the content.
/// Lengths are logical pixels; the shape is rendered at the window's scale.
pub fn tide_slash(slash: Slash) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let scale = window.scale_factor();
            let width = (f32::from(bounds.size.width) * scale).round() as u32;
            let height = (f32::from(bounds.size.height) * scale).round() as u32;
            if width == 0 || height == 0 {
                return;
            }
            let physical = Slash {
                radius: slash.radius * scale,
                edge_width: slash.edge_width * scale,
                ..slash.clone()
            };
            let key = physical.key(width, height);
            let image = CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                if let Some(image) = cache.get(&key) {
                    return image.clone();
                }
                if cache.len() >= CACHE_LIMIT {
                    cache.clear();
                }
                let pixels = rasterize(width, height, &physical);
                let image = image::RgbaImage::from_raw(width, height, pixels)
                    .map(|buffer| Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])));
                if let Some(image) = &image {
                    cache.insert(key, image.clone());
                }
                image.unwrap_or_else(|| Arc::new(RenderImage::new(Vec::new())))
            });
            if let Err(err) =
                window.paint_image(bounds, bounds, Corners::default(), image, 0, false)
            {
                tracing::debug!(error = %err, "tide slash not painted");
            }
        },
    )
    .absolute()
    .inset_0()
}

/// The colour of a CSS linear gradient (`angle` in degrees, 0 = to top) at a point of the box.
fn paint_at(paint: &Paint, x: f32, y: f32, width: f32, height: f32) -> Rgba {
    let (angle, stops) = match paint {
        Paint::Solid(c) => return *c,
        Paint::Linear { angle, stops } => (*angle, stops),
    };
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return Rgba::rgb(0, 0, 0).with_alpha(0.0);
    };
    let rad = angle.to_radians();
    let (dx, dy) = (rad.sin(), -rad.cos());
    let length = (width * dx).abs() + (height * dy).abs();
    let t = if length > 0.0 {
        (((x - width / 2.0) * dx + (y - height / 2.0) * dy) / length + 0.5).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Evenly spaced stops, as the theme syntax defines them.
    let segments = (stops.len() - 1).max(1) as f32;
    let position = t * segments;
    let ix = (position.floor() as usize).min(stops.len().saturating_sub(2));
    let (a, b) = if stops.len() == 1 {
        (first, last)
    } else {
        (&stops[ix], &stops[ix + 1])
    };
    let f = (position - ix as f32).clamp(0.0, 1.0);
    let lerp = |p: f32, q: f32| p + (q - p) * f;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

/// Straight-alpha BGRA pixels of the pill.
fn rasterize(width: u32, height: u32, s: &Slash) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    let slant = h * s.angle_deg.to_radians().tan();
    // Unit normal of the cut edge (from (slant, 0) to (0, h)), pointing into the shape.
    let norm = (h * h + slant * slant).sqrt().max(f32::EPSILON);
    let (nx, ny) = (h / norm, slant / norm);
    let r = s.radius.min(h / 2.0).max(0.0);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for py in 0..height {
        for px in 0..width {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            // Signed distance inside the cut edge (positive = inside).
            let inside_cut = (x - slant) * nx + y * ny;
            let cut = (inside_cut + 0.5).clamp(0.0, 1.0);
            // Rounded right corners, straight top and bottom.
            let right = if x > w - r && (y < r || y > h - r) {
                let cy = if y < r { r } else { h - r };
                let d = ((x - (w - r)).powi(2) + (y - cy).powi(2)).sqrt() - r;
                (0.5 - d).clamp(0.0, 1.0)
            } else {
                (w - x + 0.5).clamp(0.0, 1.0)
            };
            let coverage = cut.min(right);
            let color = if inside_cut < s.edge_width {
                s.edge
            } else {
                paint_at(&s.fill, x, y, w, h)
            };
            let alpha = (color.a * coverage).clamp(0.0, 1.0);
            let to_byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend([
                to_byte(color.b),
                to_byte(color.g),
                to_byte(color.r),
                to_byte(alpha),
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slash() -> Slash {
        Slash {
            fill: Paint::Linear {
                angle: 90.0,
                stops: vec![Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255)],
            },
            edge: Rgba::rgb(0, 255, 0),
            angle_deg: 12.0,
            radius: 6.0,
            edge_width: 3.0,
        }
    }

    fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn shape_edge_and_gradient() {
        let (w, h) = (200, 40);
        let buf = rasterize(w, h, &slash());
        // Top-left corner is cut away, bottom-left is inside the edge line.
        assert_eq!(pixel(&buf, w, 0, 0)[3], 0);
        assert_eq!(pixel(&buf, w, 1, h - 1), [0, 255, 0, 255]);
        // A left-to-right gradient: red near the start, blue near the end.
        let left = pixel(&buf, w, 20, h / 2);
        let right = pixel(&buf, w, w - 20, h / 2);
        assert!(
            left[2] > left[0] && right[0] > right[2],
            "{left:?} {right:?}"
        );
        // Rounded right corner is transparent at the very corner.
        assert_eq!(pixel(&buf, w, w - 1, 0)[3], 0);
    }

    #[test]
    fn keys_follow_inputs() {
        let a = slash().key(10, 10);
        let mut other = slash();
        other.edge = Rgba::rgb(1, 2, 3);
        assert_ne!(a, other.key(10, 10));
        assert_eq!(a, slash().key(10, 10));
    }
}
