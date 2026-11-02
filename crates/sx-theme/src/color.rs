//! Colour math: sRGB ↔ OKLab/OKLCH (Björn Ottosson's matrices), gamut mapping by chroma
//! reduction, alpha compositing and WCAG 2 contrast.
// The formulas use the published single-letter names (r, g, b, l, m, s, a).
#![allow(clippy::many_single_char_names)]

/// Non-premultiplied sRGB colour, components in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// Lightness 0..=1, chroma ≥ 0, hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

/// Ink: the navy used for text on bright highlights (DESIGN.md §2).
pub const INK: Rgba = Rgba::rgb(10, 26, 47);
pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
pub const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// `#RRGGBB` or `#RRGGBBAA`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits = hex.strip_prefix('#')?;
        if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
        Some(Self {
            a: f32::from(alpha) / 255.0,
            ..Self::rgb(byte(0)?, byte(2)?, byte(4)?)
        })
    }

    pub fn to_hex(self) -> String {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02X}{:02X}{:02X}", q(self.r), q(self.g), q(self.b))
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                q(self.r),
                q(self.g),
                q(self.b),
                q(self.a)
            )
        }
    }

    #[must_use]
    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Source-over compositing onto `background` (premultiply, add, un-premultiply).
    #[must_use]
    pub fn over(self, background: Self) -> Self {
        let a = self.a + background.a * (1.0 - self.a);
        if a <= f32::EPSILON {
            return TRANSPARENT;
        }
        let mix = |fg: f32, bg: f32| (fg * self.a + bg * background.a * (1.0 - self.a)) / a;
        Self {
            r: mix(self.r, background.r),
            g: mix(self.g, background.g),
            b: mix(self.b, background.b),
            a,
        }
    }

    /// WCAG 2 relative luminance (alpha ignored; composite first).
    pub fn luminance(self) -> f32 {
        0.2126 * to_linear(self.r) + 0.7152 * to_linear(self.g) + 0.0722 * to_linear(self.b)
    }

    pub fn to_oklab(self) -> Oklab {
        let (r, g, b) = (to_linear(self.r), to_linear(self.g), to_linear(self.b));
        let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        Oklab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }

    pub fn to_oklch(self) -> Oklch {
        self.to_oklab().to_oklch()
    }
}

/// WCAG contrast ratio between two opaque colours (1..=21).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

impl Oklab {
    pub fn to_oklch(self) -> Oklch {
        let c = self.a.hypot(self.b);
        let h = if c < 1e-6 {
            0.0
        } else {
            self.b.atan2(self.a).to_degrees().rem_euclid(360.0)
        };
        Oklch { l: self.l, c, h }
    }

    /// Linear sRGB without clamping (components may leave 0..=1 when out of gamut).
    fn to_linear_rgb(self) -> [f32; 3] {
        let l = (self.l + 0.396_337_78 * self.a + 0.215_803_76 * self.b).powi(3);
        let m = (self.l - 0.105_561_346 * self.a - 0.063_854_17 * self.b).powi(3);
        let s = (self.l - 0.089_484_18 * self.a - 1.291_485_5 * self.b).powi(3);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }
}

impl Oklch {
    pub fn to_oklab(self) -> Oklab {
        let (sin, cos) = self.h.to_radians().sin_cos();
        Oklab {
            l: self.l,
            a: self.c * cos,
            b: self.c * sin,
        }
    }

    /// Whether the colour fits in sRGB.
    pub fn in_gamut(self) -> bool {
        self.to_oklab()
            .to_linear_rgb()
            .iter()
            .all(|v| (-1e-4..=1.0 + 1e-4).contains(v))
    }

    /// Converts to sRGB, lowering chroma (keeping L and h) until the colour fits.
    pub fn to_rgba(self, alpha: f32) -> Rgba {
        let l = self.l.clamp(0.0, 1.0);
        let mut color = Self { l, ..self };
        if !color.in_gamut() {
            let (mut lo, mut hi) = (0.0_f32, color.c);
            for _ in 0..24 {
                let mid = f32::midpoint(lo, hi);
                if (Self { c: mid, ..color }).in_gamut() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            color.c = lo;
        }
        let [r, g, b] = color.to_oklab().to_linear_rgb();
        Rgba {
            r: from_linear(r.clamp(0.0, 1.0)),
            g: from_linear(g.clamp(0.0, 1.0)),
            b: from_linear(b.clamp(0.0, 1.0)),
            a: alpha.clamp(0.0, 1.0),
        }
    }
}

/// Mix in OKLab, `t` = share of `b` (`0..=1`). Alpha mixes linearly.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let (x, y) = (a.to_oklab(), b.to_oklab());
    let lerp = |p: f32, q: f32| p + (q - p) * t;
    Oklab {
        l: lerp(x.l, y.l),
        a: lerp(x.a, y.a),
        b: lerp(x.b, y.b),
    }
    .to_oklch()
    .to_rgba(lerp(a.a, b.a))
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn hex_round_trip() {
        let c = Rgba::from_hex("#2FA8FF").unwrap();
        assert_eq!(c.to_hex(), "#2FA8FF");
        assert_eq!(Rgba::from_hex("#0A1A2F80").unwrap().to_hex(), "#0A1A2F80");
        assert!(Rgba::from_hex("#12345").is_none());
        assert!(Rgba::from_hex("2FA8FF").is_none());
    }

    #[test]
    fn oklch_round_trip() {
        for hex in [
            "#2FA8FF", "#0A1A2F", "#FFFFFF", "#000000", "#C42B1C", "#5CE1FF",
        ] {
            let c = Rgba::from_hex(hex).unwrap();
            let back = c.to_oklch().to_rgba(1.0);
            assert_eq!(back.to_hex(), hex, "round trip of {hex}");
        }
    }

    #[test]
    fn known_oklab_values() {
        // White is L=1 with no chroma; black is L=0.
        let white = WHITE.to_oklch();
        assert!(close(white.l, 1.0, 1e-3) && white.c < 1e-3);
        assert!(close(Rgba::rgb(0, 0, 0).to_oklch().l, 0.0, 1e-4));
    }

    #[test]
    fn wcag_contrast() {
        assert!(close(contrast(WHITE, Rgba::rgb(0, 0, 0)), 21.0, 1e-3));
        // #767676 on white is the classic 4.54:1.
        assert!(close(
            contrast(WHITE, Rgba::rgb(0x76, 0x76, 0x76)),
            4.54,
            0.01
        ));
    }

    #[test]
    fn compositing() {
        let half_black = Rgba::rgb(0, 0, 0).with_alpha(0.5);
        let c = half_black.over(WHITE);
        assert!(close(c.r, 0.5, 1e-3) && close(c.a, 1.0, 1e-6));
        assert_eq!(TRANSPARENT.over(TRANSPARENT), TRANSPARENT);
    }

    #[test]
    fn gamut_mapping_keeps_lightness() {
        let wild = Oklch {
            l: 0.7,
            c: 0.5,
            h: 240.0,
        };
        assert!(!wild.in_gamut());
        let mapped = wild.to_rgba(1.0).to_oklch();
        assert!(close(mapped.l, 0.7, 0.01));
        assert!(close(mapped.h, 240.0, 3.0));
    }
}
