//! Contrast validation (THEMING.md §11). Each pair is checked over the worst backdrops of the
//! variant: the `under` layers are composited first (e.g. a selected row sits on `bg.content`),
//! then every stop of the background, then every stop of the foreground; the lowest ratio counts.
//! Text needs 4.5:1, state indicators and control boundaries 3:1 (WCAG 1.4.3 / 1.4.11).

use crate::color::{Rgba, contrast};
use crate::resolve::{Mode, ResolvedTheme};
use crate::schema::Token::{
    self, AccentSolid, AccentSolidHover, AccentSolidPressed, AccentText, BgContent, BgSidebar,
    BgStatusbar, BgTitlebar, BgWindow, BorderInput, BorderInputFocus, BorderInputHover,
    BreadcrumbCurrentFg, BreadcrumbFg, ButtonDangerBg, ButtonDangerBgHover, ButtonDangerFg,
    ButtonGhostBgHover, ButtonGhostBgPressed, ButtonGhostFgHover, ButtonPrimaryBg,
    ButtonPrimaryBgHover, ButtonPrimaryBgPressed, ButtonPrimaryFg, ButtonSecondaryBg,
    ButtonSecondaryBgHover, ButtonSecondaryBgPressed, ButtonSecondaryFg, ButtonSecondaryFgHover,
    CardHoverFg, CheckboxBorder, CheckboxCheck, CheckboxCheckedBg, DangerText, DropBlockedBorder,
    DropTargetBg, DropTargetBorder, FgLink, FgMuted, FgOnSolid, FgPrimary, FgSecondary, FocusRing,
    HeaderFg, HoverBg, InfoText, InputPlaceholder, MatchFg, NavItemActiveBg, NavItemActiveFg,
    NavItemFg, NavItemHoverBg, NavItemHoverFg, NavItemPressedBg, NavSectionFg, OsdBg, OsdFg,
    OsdIconOff, OsdIconOn, OsdStateOff, OsdStateOn, PressedBg, ProgressFill, RowHoverBg,
    RowSelectedBg, RowSelectedBorder, RowSelectedInactiveBg, SelectionBorder, SliderFill,
    StatuschipOff, StatuschipOn, StatuschipPartial, SuccessText, SurfaceCard, SurfaceCardHover,
    SurfaceChip, SurfaceInput, SurfaceOverlay, TabActiveBg, TabActiveFg, TabActiveIndicator, TabFg,
    ToastBg, ToastFg, ToggleOffBorder, ToggleOffThumb, ToggleOnBg, ToggleOnThumb, WarningText,
};

/// A foreground/background requirement.
#[derive(Clone, Copy, Debug)]
pub struct ContrastPair {
    pub fg: &'static [Token],
    pub bg: &'static [Token],
    /// Layers below `bg`, bottom first (composited over the backdrop before `bg`).
    pub under: &'static [Token],
    pub min: f32,
}

const TEXT: f32 = 4.5;
const NON_TEXT: f32 = 3.0;

const fn pair(
    fg: &'static [Token],
    bg: &'static [Token],
    under: &'static [Token],
    min: f32,
) -> ContrastPair {
    ContrastPair { fg, bg, under, min }
}

pub const PAIRS: &[ContrastPair] = &[
    // Text on surfaces.
    pair(
        &[FgPrimary, FgSecondary, FgMuted],
        &[BgContent, SurfaceCard, SurfaceOverlay],
        &[],
        TEXT,
    ),
    pair(
        &[FgPrimary, FgSecondary, FgMuted],
        &[RowSelectedBg, RowSelectedInactiveBg],
        &[BgContent],
        TEXT,
    ),
    pair(&[HeaderFg], &[BgContent], &[], TEXT),
    // The command palette: text and matches on the overlay and on its rows.
    pair(
        &[FgPrimary, FgSecondary, FgMuted, MatchFg],
        &[SurfaceOverlay],
        &[BgContent],
        TEXT,
    ),
    pair(
        &[FgPrimary, FgSecondary, FgMuted, MatchFg],
        &[RowHoverBg, RowSelectedBg],
        &[BgContent, SurfaceOverlay],
        TEXT,
    ),
    pair(
        &[RowSelectedBorder],
        &[SurfaceOverlay],
        &[BgContent],
        NON_TEXT,
    ),
    // The Hot Switch panel's toggles, on the overlay and on its rows.
    pair(
        &[ToggleOffBorder, ToggleOffThumb, ToggleOnBg],
        &[SurfaceOverlay],
        &[BgContent],
        NON_TEXT,
    ),
    pair(
        &[ToggleOffBorder, ToggleOffThumb, ToggleOnBg],
        &[RowHoverBg, RowSelectedBg, PressedBg],
        &[BgContent, SurfaceOverlay],
        NON_TEXT,
    ),
    // A drag over a folder row, the content pane or a sidebar item (DESIGN.md §11).
    pair(
        &[FgPrimary, FgSecondary, FgMuted, MatchFg],
        &[DropTargetBg],
        &[BgContent],
        TEXT,
    ),
    pair(&[NavItemFg, FgMuted], &[DropTargetBg], &[BgSidebar], TEXT),
    pair(
        &[DropTargetBorder, DropBlockedBorder],
        &[BgSidebar],
        &[],
        NON_TEXT,
    ),
    // The context menu: its rows under the pointer and pressed.
    pair(
        &[FgPrimary, FgSecondary],
        &[HoverBg, PressedBg],
        &[BgContent, SurfaceOverlay],
        TEXT,
    ),
    // The matching part of a name, on every row state.
    pair(&[MatchFg], &[BgContent], &[], TEXT),
    pair(
        &[MatchFg],
        &[RowHoverBg, RowSelectedBg, RowSelectedInactiveBg],
        &[BgContent],
        TEXT,
    ),
    // Chrome text: command bar buttons (bg.window), status bar, sidebar captions.
    pair(
        &[FgPrimary, FgSecondary],
        &[BgWindow, BgStatusbar],
        &[],
        TEXT,
    ),
    pair(&[FgPrimary, FgSecondary, FgMuted], &[BgSidebar], &[], TEXT),
    pair(&[NavItemFg, NavSectionFg], &[BgSidebar], &[], TEXT),
    pair(&[NavItemActiveFg], &[NavItemActiveBg], &[BgSidebar], TEXT),
    pair(&[TabFg], &[BgTitlebar], &[], TEXT),
    pair(&[TabActiveFg], &[TabActiveBg], &[], TEXT),
    pair(&[BreadcrumbFg, BreadcrumbCurrentFg], &[BgWindow], &[], TEXT),
    pair(
        &[StatuschipOn, StatuschipPartial, StatuschipOff],
        &[BgStatusbar],
        &[],
        TEXT,
    ),
    pair(
        &[StatuschipOn, StatuschipPartial, StatuschipOff],
        &[SurfaceChip],
        &[BgStatusbar],
        TEXT,
    ),
    // The chip is a button: under the pointer and pressed.
    pair(
        &[StatuschipOn, StatuschipPartial, StatuschipOff],
        &[HoverBg, PressedBg],
        &[BgStatusbar],
        TEXT,
    ),
    pair(&[OsdFg], &[OsdBg], &[], TEXT),
    pair(&[ToastFg], &[ToastBg], &[], TEXT),
    pair(&[InputPlaceholder], &[SurfaceInput], &[], TEXT),
    pair(
        &[
            AccentText,
            FgLink,
            SuccessText,
            WarningText,
            DangerText,
            InfoText,
        ],
        &[BgContent],
        &[],
        TEXT,
    ),
    // Text on filled controls.
    pair(
        &[ButtonPrimaryFg],
        &[
            ButtonPrimaryBg,
            ButtonPrimaryBgHover,
            ButtonPrimaryBgPressed,
        ],
        &[],
        TEXT,
    ),
    pair(
        &[ButtonSecondaryFg],
        &[ButtonSecondaryBg],
        &[BgContent],
        TEXT,
    ),
    // Pointer over and pressed: the highlight gradient with Ink text.
    pair(
        &[ButtonSecondaryFgHover],
        &[ButtonSecondaryBgHover, ButtonSecondaryBgPressed],
        &[BgContent],
        TEXT,
    ),
    pair(
        &[ButtonGhostFgHover],
        &[ButtonGhostBgHover, ButtonGhostBgPressed],
        &[BgWindow],
        TEXT,
    ),
    pair(
        &[NavItemHoverFg],
        &[NavItemHoverBg, NavItemPressedBg],
        &[BgSidebar],
        TEXT,
    ),
    pair(&[CardHoverFg], &[SurfaceCardHover], &[BgContent], TEXT),
    pair(
        &[FgOnSolid],
        &[AccentSolid, AccentSolidHover, AccentSolidPressed],
        &[],
        TEXT,
    ),
    pair(
        &[ButtonDangerFg],
        &[ButtonDangerBg, ButtonDangerBgHover],
        &[],
        TEXT,
    ),
    // Boundaries and state indicators (WCAG 1.4.11).
    pair(
        &[FocusRing, BorderInput, BorderInputFocus, BorderInputHover],
        &[BgContent, SurfaceInput],
        &[],
        NON_TEXT,
    ),
    // The address field sits on the command bar; focus rings also land on chrome surfaces.
    pair(
        &[BorderInput, BorderInputFocus, BorderInputHover],
        &[BgWindow],
        &[],
        NON_TEXT,
    ),
    pair(
        &[FocusRing],
        &[BgSidebar, BgTitlebar, BgWindow],
        &[],
        NON_TEXT,
    ),
    // The ring goes around controls on dialogs, toasts and cards too.
    pair(
        &[FocusRing],
        &[SurfaceOverlay, ToastBg, SurfaceCard],
        &[BgContent],
        NON_TEXT,
    ),
    // Hot Switch: the OSD caption, and the "Drag off" badge in the title bar.
    pair(&[FgSecondary], &[OsdBg], &[BgContent], TEXT),
    pair(&[StatuschipOff], &[BgTitlebar], &[], TEXT),
    pair(
        &[CheckboxBorder, ToggleOffBorder, SelectionBorder],
        &[BgContent],
        &[],
        NON_TEXT,
    ),
    pair(
        &[DropTargetBorder, DropBlockedBorder],
        &[BgContent],
        &[],
        NON_TEXT,
    ),
    pair(
        &[SliderFill, ProgressFill, ToggleOnBg],
        &[BgContent],
        &[],
        NON_TEXT,
    ),
    pair(&[TabActiveIndicator], &[BgTitlebar], &[], NON_TEXT),
    pair(
        &[CheckboxCheck],
        &[CheckboxCheckedBg],
        &[BgContent],
        NON_TEXT,
    ),
    pair(&[ToggleOnThumb], &[ToggleOnBg], &[BgContent], NON_TEXT),
    pair(&[OsdIconOn], &[OsdStateOn], &[OsdBg], NON_TEXT),
    pair(&[OsdIconOff], &[OsdStateOff], &[OsdBg], NON_TEXT),
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

/// The lowest contrast of `fg` over `bg` (itself over `under`) across backdrops and stops.
pub fn worst_contrast(theme: &ResolvedTheme, fg: Token, bg: Token, under: &[Token]) -> f32 {
    let mut worst = f32::INFINITY;
    for backdrop in backdrops(theme.mode) {
        let base = under.iter().fold(backdrop, |base, layer| {
            theme.paint(*layer).average().over(base)
        });
        for stop in theme.paint(bg).colors() {
            let surface = stop.over(base);
            for text in theme.paint(fg).colors() {
                worst = worst.min(contrast(text.over(surface), surface));
            }
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
                let ratio = worst_contrast(theme, fg, bg, pair.under);
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
        assert!(worst_contrast(&theme, NavItemActiveFg, NavItemActiveBg, &[]) < 2.0);
    }

    #[test]
    fn layers_below_change_the_result() {
        let mut file = ThemeFile::parse(builtin::CERULEAN).unwrap();
        // A translucent black selection darkens whatever is under it.
        file.light
            .insert("row.selected.bg".into(), "alpha(#000000, 0.5)".into());
        let theme = resolve(&file, Mode::Light, &ResolveOptions::default()).unwrap();
        let alone = worst_contrast(&theme, FgPrimary, RowSelectedBg, &[]);
        let on_content = worst_contrast(&theme, FgPrimary, RowSelectedBg, &[BgContent]);
        assert!((alone - on_content).abs() > 0.01);
    }
}
