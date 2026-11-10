//! Theme tokens in the UI (THEMING.md §8, §14): the resolved Switch Explorer theme lives in the
//! [`Tokens`] global, read with `cx.tokens()`, and is bridged into gpui-component's `Theme` so its
//! controls use the same colours.

use std::sync::Arc;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App, Background, BorrowAppContext as _, BoxShadow, Global, Hsla, Pixels, Rgba, SharedString,
    linear_color_stop, linear_gradient, point, px,
};
use sx_theme::{Mode, Paint, ResolveOptions, ResolvedTheme, ThemeFile, Token, builtin};

/// GPUI draws `border_1()` one pixel wide; sizes that include a border are worked out with it.
pub(crate) const HAIRLINE: f32 = 1.0;

/// The theme the UI is drawn with.
#[derive(Debug)]
pub struct Tokens {
    theme: Arc<ResolvedTheme>,
    colors: Vec<Hsla>,
    paints: Vec<Background>,
    /// Font family for UI text (the first installed one of `[fonts].ui`).
    pub ui_font: SharedString,
}

impl Global for Tokens {}

pub trait ActiveTokens {
    fn tokens(&self) -> &Tokens;
}

impl ActiveTokens for App {
    fn tokens(&self) -> &Tokens {
        self.global::<Tokens>()
    }
}

pub fn hsla(c: sx_theme::Rgba) -> Hsla {
    Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

/// GPUI gradients have two stops: the first and last stop of a longer gradient are used.
fn background(paint: &Paint) -> Background {
    match paint {
        Paint::Solid(c) => hsla(*c).into(),
        Paint::Linear { angle, stops } => match (stops.first(), stops.last()) {
            (Some(from), Some(to)) => linear_gradient(
                *angle,
                linear_color_stop(hsla(*from), 0.0),
                linear_color_stop(hsla(*to), 1.0),
            ),
            _ => gpui_kit::transparent_black().into(),
        },
    }
}

impl Tokens {
    fn new(theme: ResolvedTheme, ui_font: SharedString) -> Self {
        let colors = sx_theme::TOKENS
            .iter()
            .map(|m| hsla(theme.color(m.token)))
            .collect();
        let paints = sx_theme::TOKENS
            .iter()
            .map(|m| background(theme.paint(m.token)))
            .collect();
        Self {
            theme: Arc::new(theme),
            colors,
            paints,
            ui_font,
        }
    }

    pub fn mode(&self) -> Mode {
        self.theme.mode
    }

    pub fn resolved(&self) -> &ResolvedTheme {
        &self.theme
    }

    /// A colour token (gradients give their average).
    pub fn color(&self, token: Token) -> Hsla {
        self.colors[token as usize]
    }

    /// A paint token as a GPUI background (gradients included).
    pub fn paint(&self, token: Token) -> Background {
        self.paints[token as usize]
    }

    pub fn number(&self, token: Token) -> f32 {
        self.theme.number(token)
    }

    pub fn px(&self, token: Token) -> Pixels {
        px(self.theme.number(token))
    }

    pub fn shadow(&self, token: Token) -> Vec<BoxShadow> {
        self.theme
            .shadow(token)
            .map(|s| BoxShadow {
                color: hsla(s.color),
                offset: point(px(s.x), px(s.y)),
                blur_radius: px(s.blur),
                spread_radius: px(s.spread),
                inset: false,
            })
            .into_iter()
            .collect()
    }
}

/// Everything that decides how the theme resolves, kept so it can be re-resolved when the OS
/// switches light/dark or transparency.
#[derive(Debug)]
pub struct ThemeController {
    file: ThemeFile,
    options: ResolveOptions,
    mode: Mode,
}

impl Global for ThemeController {}

impl ThemeController {
    /// The built-in Cerulean theme. It is validated in CI, so parsing cannot fail at run time;
    /// an error is still reported instead of panicking.
    pub fn builtin() -> Result<Self, sx_theme::ThemeError> {
        Ok(Self {
            file: ThemeFile::parse(builtin::CERULEAN)?,
            options: ResolveOptions::default(),
            mode: Mode::Light,
        })
    }

    /// Installs the controller and the first theme.
    pub fn install(self, mode: Mode, solid: bool, cx: &mut App) {
        cx.set_global(self);
        Self::apply(mode, solid, cx);
    }

    /// Re-resolves for `mode` and the given material and installs the result: our [`Tokens`]
    /// and gpui-component's colours. Call after every appearance change. Does nothing before
    /// [`Self::install`].
    pub fn apply(mode: Mode, solid: bool, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let resolved = cx.update_global::<Self, _>(|ctl, _| {
            ctl.mode = mode;
            ctl.options.solid = solid;
            sx_theme::resolve(&ctl.file, mode, &ctl.options)
        });
        let resolved = match resolved {
            Ok(theme) => theme,
            Err(err) => {
                tracing::error!(error = %err, "theme does not resolve; keeping the previous one");
                return;
            }
        };
        for warning in &resolved.warnings {
            tracing::warn!(key = %warning.key, message = %warning.message, "theme warning");
        }
        let ui_font = pick_font(&resolved.fonts.ui, cx);
        let tokens = Tokens::new(resolved, ui_font);
        bridge(&tokens, solid, cx);
        cx.set_global(tokens);
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn solid(&self) -> bool {
        self.options.solid
    }
}

/// The first installed family of a theme's font list, or the system UI font.
fn pick_font(families: &[String], cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    families
        .iter()
        .find(|f| installed.iter().any(|i| i.eq_ignore_ascii_case(f)))
        .map_or_else(|| SharedString::from(".SystemUIFont"), |f| f.clone().into())
}

/// Fills gpui-component's colours from our tokens (THEMING.md §14). The window background stays
/// transparent over Mica (each region paints its own `bg.*` token); with solid material it is the
/// opaque `bg.window`.
fn bridge(tokens: &Tokens, solid: bool, cx: &mut App) {
    let mode = match tokens.mode() {
        Mode::Light => ThemeMode::Light,
        Mode::Dark => ThemeMode::Dark,
    };
    // Loads the registered theme for the mode first; our colours go on top in a second update.
    Theme::change(mode, None, cx);
    let c = |t| tokens.color(t);
    let font = tokens.ui_font.clone();
    let radius = tokens.px(Token::RadiusSm);
    let radius_lg = tokens.px(Token::RadiusLg);
    let font_size = tokens.px(Token::FontSizeBody);
    Theme::update(cx, |theme| {
        theme.font_family = font;
        theme.font_size = font_size;
        theme.radius = radius;
        theme.radius_lg = radius_lg;
        let colors = &mut theme.colors;
        colors.background = if solid {
            c(Token::BgWindow)
        } else {
            gpui_kit::transparent_black()
        };
        colors.foreground = c(Token::FgPrimary);
        colors.border = c(Token::BorderSubtle);
        colors.input = c(Token::BorderInput);
        colors.ring = c(Token::FocusRing);
        colors.caret = c(Token::FgPrimary);
        colors.selection = c(Token::RowSelectedBg);
        colors.primary = c(Token::ButtonPrimaryBg);
        colors.primary_hover = c(Token::ButtonPrimaryBgHover);
        colors.primary_active = c(Token::ButtonPrimaryBgPressed);
        colors.primary_foreground = c(Token::ButtonPrimaryFg);
        colors.secondary = c(Token::ButtonSecondaryBg);
        colors.secondary_hover = c(Token::ButtonSecondaryBgHover);
        colors.secondary_active = c(Token::PressedBg);
        colors.secondary_foreground = c(Token::ButtonSecondaryFg);
        colors.accent = c(Token::HoverBg);
        colors.accent_foreground = c(Token::FgPrimary);
        colors.muted = c(Token::SurfaceChip);
        colors.muted_foreground = c(Token::FgMuted);
        colors.list = gpui_kit::transparent_black();
        colors.list_hover = c(Token::RowHoverBg);
        colors.list_active = c(Token::RowSelectedBg);
        colors.list_active_border = c(Token::RowSelectedBorder);
        colors.table = c(Token::BgContent);
        colors.table_hover = c(Token::RowHoverBg);
        colors.table_active = c(Token::RowSelectedBg);
        colors.table_active_border = c(Token::RowSelectedBorder);
        colors.table_head_foreground = c(Token::HeaderFg);
        colors.sidebar = c(Token::BgSidebar);
        colors.sidebar_foreground = c(Token::NavItemFg);
        colors.title_bar = c(Token::BgTitlebar);
        colors.tab_active = c(Token::TabActiveBg);
        colors.tab_foreground = c(Token::TabFg);
        colors.tab_active_foreground = c(Token::TabActiveFg);
        colors.popover = c(Token::SurfaceOverlay);
        colors.popover_foreground = c(Token::FgPrimary);
        colors.overlay = c(Token::Scrim);
        colors.scrollbar = c(Token::ScrollbarTrack);
        colors.scrollbar_thumb = c(Token::ScrollbarThumb);
        colors.scrollbar_thumb_hover = c(Token::ScrollbarThumbHover);
        colors.link = c(Token::FgLink);
        colors.danger = c(Token::DangerSolid);
        colors.success = c(Token::SuccessSolid);
        colors.warning = c(Token::WarningSolid);
        colors.info = c(Token::InfoSolid);
        colors.progress_bar = c(Token::ProgressFill);
        colors.slider_bar = c(Token::SliderFill);
        colors.switch = c(Token::ToggleOnBg);
        colors.status_bar = c(Token::BgStatusbar);
        colors.drop_target = c(Token::DropTargetBg);
        colors.drag_border = c(Token::DropTargetBorder);
    });
}
