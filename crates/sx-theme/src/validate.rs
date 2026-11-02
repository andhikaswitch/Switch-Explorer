//! Contrast validation (THEMING.md §11). Translucent backgrounds are composited onto the worst
//! backdrop of the variant and every gradient stop is checked; the lowest ratio counts.

use crate::color::{Rgba, contrast};
use crate::resolve::{Mode, ResolvedTheme};
use crate::schema::Token::{
    self, AccentSolid, AccentSolidHover, AccentSolidPressed, AccentText, BgContent, BorderInput,
    BorderInputFocus, ButtonDangerBg, ButtonDangerBgHover, ButtonDangerFg, ButtonPrimaryBg,
    ButtonPrimaryBgHover, ButtonPrimaryBgPressed, ButtonPrimaryFg, FgLink, FgMuted, FgOnSolid,
    FgPrimary, FgSecondary, FocusRing, NavItemActiveBg, NavItemActiveFg, OsdIconOn, OsdStateOn,
    SurfaceCard, SurfaceInput, SurfaceOverlay,
};

/// A foreground/background requirement.
#[derive(Clone, Copy, Debug)]
pub struct ContrastPair {
    pub fg: &'static [Token],
    pub bg: &'static [Token],
    pub min: f32,
}

pub const PAIRS: &[ContrastPair] = &[
    ContrastPair {
        fg: &[FgPrimary, FgSecondary, FgMuted],
        bg: &[BgContent, SurfaceCard, SurfaceOverlay],
        min: 4.5,
    },
    ContrastPair {
        fg: &[NavItemActiveFg],
        bg: &[NavItemActiveBg],
        min: 4.5,
    },
    ContrastPair {
        fg: &[ButtonPrimaryFg],
        bg: &[
            ButtonPrimaryBg,
            ButtonPrimaryBgHover,
            ButtonPrimaryBgPressed,
        ],
        min: 4.5,
    },
    ContrastPair {
        fg: &[FgOnSolid],
        bg: &[AccentSolid, AccentSolidHover, AccentSolidPressed],
        min: 4.5,
    },
    ContrastPair {
        fg: &[ButtonDangerFg],
        bg: &[ButtonDangerBg, ButtonDangerBgHover],
        min: 4.5,
    },
    ContrastPair {
        fg: &[AccentText, FgLink],
        bg: &[BgContent],
        min: 4.5,
    },
    ContrastPair {
        fg: &[FocusRing, BorderInput, BorderInputFocus],
        bg: &[BgContent, SurfaceInput],
        min: 3.0,
    },
    ContrastPair {
        fg: &[OsdIconOn],
        bg: &[OsdStateOn],
        min: 3.0,
    },
];

/// Worst-case backdrops behind a translucent window (Mica can be anywhere in this range).
pub fn backdrops(mode: Mode) -> [Rgba; 2] {
    match mode {
        Mode::Light => [Rgba::rgb(0xFF, 0xFF, 0xFF), Rgba::rgb(0xE9, 0xE9, 0xE9)],
        Mode::Dark => [Rgba::rgb(0x00, 0x00, 0x00), Rgba::rgb(0x2B, 0x2B, 0x2B)],
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContrastIssue {
    pub fg: Token,
    pub bg: Token,
    pub ratio: f32,
    pub min: f32,
}

/// The lowest contrast of `fg` over `bg` across backdrops and gradient stops.
pub fn worst_contrast(theme: &ResolvedTheme, fg: Token, bg: Token) -> f32 {
    let text = theme.color(fg);
    let mut worst = f32::INFINITY;
    for backdrop in backdrops(theme.mode) {
        for stop in theme.paint(bg).colors() {
            let surface = stop.over(backdrop);
            worst = worst.min(contrast(text.over(surface), surface));
        }
    }
    worst
}

/// Every pair below its minimum. An empty list means the theme passes.
pub fn check(theme: &ResolvedTheme) -> Vec<ContrastIssue> {
    let mut issues = Vec::new();
    for pair in PAIRS {
        for &fg in pair.fg {
            for &bg in pair.bg {
                let ratio = worst_contrast(theme, fg, bg);
                if ratio + 1e-3 < pair.min {
                    issues.push(ContrastIssue {
                        fg,
                        bg,
                        ratio,
                        min: pair.min,
                    });
                }
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResolveOptions, ThemeFile, builtin, resolve};

    #[test]
    fn catches_poor_contrast() {
        let mut file = ThemeFile::parse(builtin::CERULEAN).unwrap();
        file.light.insert("fg.muted".into(), "#E0E0E0".into());
        let theme = resolve(&file, Mode::Light, &ResolveOptions::default()).unwrap();
        let issues = check(&theme);
        assert!(
            issues
                .iter()
                .any(|i| i.fg == FgMuted && i.bg == BgContent && i.ratio < 2.0),
            "{issues:?}"
        );
    }

    #[test]
    fn gradient_backgrounds_use_the_worst_stop() {
        let mut file = ThemeFile::parse(builtin::CERULEAN).unwrap();
        // Ink text on a gradient whose second stop is nearly black.
        file.common.insert(
            "nav.item.active.bg".into(),
            "gradient(120deg, #5CE1FF, #101010)".into(),
        );
        let theme = resolve(&file, Mode::Light, &ResolveOptions::default()).unwrap();
        assert!(worst_contrast(&theme, NavItemActiveFg, NavItemActiveBg) < 2.0);
    }
}
