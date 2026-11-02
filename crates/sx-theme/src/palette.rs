//! Palette generator (THEMING.md §9): accent and neutral ramps from one seed each, in OKLCH,
//! gamut-mapped to sRGB, with automatic contrast constraints.

use crate::color::{INK, Oklch, Rgba, WHITE, contrast};

/// Step names of a ramp (`pal.accent.500`).
pub const STEPS: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];

/// Target lightness per step; step 500 uses the seed's own L (clamped).
const TARGET_L: [f32; 11] = [
    0.975,
    0.95,
    0.90,
    0.84,
    0.77,
    f32::NAN,
    0.62,
    0.55,
    0.47,
    0.38,
    0.28,
];
const CHROMA_FACTOR: [f32; 11] = [
    0.15, 0.25, 0.45, 0.70, 0.90, 1.00, 0.95, 0.85, 0.75, 0.60, 0.45,
];
const STEP_500: usize = 5;
const STEP_700: usize = 7;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub accent: [Rgba; 11],
    pub neutral: [Rgba; 11],
    pub lagoon: Rgba,
}

impl Palette {
    pub fn generate(accent_seed: Rgba, neutral_seed: Rgba) -> Self {
        let seed = accent_seed.to_oklch();
        let mut accent = ramp(seed);
        // 700 carries text on white: at least 4.5:1, lowering L until it passes.
        accent[STEP_700] = adjust_l(accent[STEP_700], -0.01, |c| contrast(c, WHITE) >= 4.5);
        // 500 sits under Ink text: at least 4.5:1, raising L until it passes.
        accent[STEP_500] = adjust_l(accent[STEP_500], 0.01, |c| contrast(c, INK) >= 4.5);
        let lagoon = Oklch {
            l: (seed.l + 0.08).min(0.98),
            c: seed.c * 0.9,
            h: (seed.h - 12.0).rem_euclid(360.0),
        }
        .to_rgba(1.0);
        let lagoon = adjust_l(lagoon, 0.01, |c| contrast(c, INK) >= 4.5);

        let neutral_seed = neutral_seed.to_oklch();
        let neutral = ramp(Oklch {
            c: neutral_seed.c.clamp(0.02, 0.03),
            ..neutral_seed
        });
        Self {
            accent,
            neutral,
            lagoon,
        }
    }

    /// Looks up `pal.accent.<step>`, `pal.neutral.<step>` or `pal.lagoon`.
    pub fn get(&self, id: &str) -> Option<Rgba> {
        let rest = id.strip_prefix("pal.")?;
        if rest == "lagoon" {
            return Some(self.lagoon);
        }
        let (ramp, step) = rest.split_once('.')?;
        let ix = STEPS.iter().position(|s| s.to_string() == step)?;
        match ramp {
            "accent" => Some(self.accent[ix]),
            "neutral" => Some(self.neutral[ix]),
            _ => None,
        }
    }

    /// Applies `[palette.pin]` entries (`"accent.500" = "#2FA8FF"`, `"lagoon" = …`).
    pub fn pin(&mut self, key: &str, color: Rgba) -> bool {
        if key == "lagoon" {
            self.lagoon = color;
            return true;
        }
        let Some((ramp, step)) = key.split_once('.') else {
            return false;
        };
        let Some(ix) = STEPS.iter().position(|s| s.to_string() == step) else {
            return false;
        };
        match ramp {
            "accent" => self.accent[ix] = color,
            "neutral" => self.neutral[ix] = color,
            _ => return false,
        }
        true
    }
}

fn ramp(seed: Oklch) -> [Rgba; 11] {
    std::array::from_fn(|ix| {
        let l = if ix == STEP_500 {
            seed.l.clamp(0.68, 0.74)
        } else {
            TARGET_L[ix]
        };
        Oklch {
            l,
            c: seed.c * CHROMA_FACTOR[ix],
            h: seed.h,
        }
        .to_rgba(1.0)
    })
}

/// Moves L in `step` increments until `ok` holds (or L hits its bound).
fn adjust_l(color: Rgba, step: f32, ok: impl Fn(Rgba) -> bool) -> Rgba {
    let mut lch = color.to_oklch();
    let mut current = color;
    for _ in 0..100 {
        if ok(current) {
            break;
        }
        lch.l = (lch.l + step).clamp(0.0, 1.0);
        current = lch.to_rgba(color.a);
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_hold_for_many_seeds() {
        for hex in [
            "#2FA8FF", "#FF6B6B", "#7C3AED", "#10B981", "#F59E0B", "#111111", "#FAFAFA",
        ] {
            let p = Palette::generate(
                Rgba::from_hex(hex).unwrap(),
                Rgba::from_hex("#5B6B80").unwrap(),
            );
            assert!(
                contrast(p.accent[STEP_700], WHITE) >= 4.5,
                "{hex}: 700 vs white"
            );
            assert!(
                contrast(p.accent[STEP_500], INK) >= 4.5,
                "{hex}: 500 vs ink"
            );
            assert!(contrast(p.lagoon, INK) >= 4.5, "{hex}: lagoon vs ink");
        }
    }

    #[test]
    fn ramps_get_darker() {
        let p = Palette::generate(
            Rgba::from_hex("#2FA8FF").unwrap(),
            Rgba::from_hex("#5B6B80").unwrap(),
        );
        for pair in p.accent.windows(2) {
            assert!(pair[0].to_oklch().l > pair[1].to_oklch().l);
        }
        assert!(p.neutral[STEP_500].to_oklch().c <= 0.031);
    }

    #[test]
    fn lookup_and_pins() {
        let mut p = Palette::generate(
            Rgba::from_hex("#2FA8FF").unwrap(),
            Rgba::from_hex("#5B6B80").unwrap(),
        );
        assert!(p.get("pal.accent.950").is_some());
        assert!(p.get("pal.neutral.50").is_some());
        assert!(p.get("pal.accent.550").is_none());
        let pinned = Rgba::from_hex("#8FD0FF").unwrap();
        assert!(p.pin("accent.300", pinned));
        assert_eq!(p.get("pal.accent.300"), Some(pinned));
        assert!(!p.pin("accent.301", pinned));
    }
}
