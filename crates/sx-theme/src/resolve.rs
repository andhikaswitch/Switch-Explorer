//! Resolution pipeline (THEMING.md §8): palette → `[common]` → `[light]`/`[dark]` → overrides,
//! evaluated topologically with cycle detection, then environment adjustments. The result is a
//! flat array indexed by [`Token`], so lookups at render time cost nothing.
//!
//! A value that fails (bad syntax, wrong type, cycle, unknown reference) never blanks a token:
//! the next lower layer is tried, down to the theme's own value (THEMING.md §11), and each
//! failure becomes one warning.

use std::collections::{BTreeMap, BTreeSet};

use crate::color::{self, INK, Oklab, Oklch, Rgba, TRANSPARENT, WHITE, contrast};
use crate::error::{ThemeError, ThemeWarning};
use crate::file::{Fonts, Material, ThemeFile};
use crate::palette::Palette;
use crate::schema::{TOKENS, Token, TokenType};
use crate::validate::backdrops;
use crate::value::{self, Expr, Func};

/// Maximum reference depth (THEMING.md §4).
const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    Linear { angle: f32, stops: Vec<Rgba> },
}

static NO_PAINT: Paint = Paint::Solid(TRANSPARENT);

impl Paint {
    /// Every colour the paint can show (the stops of a gradient).
    pub fn colors(&self) -> &[Rgba] {
        match self {
            Self::Solid(c) => std::slice::from_ref(c),
            Self::Linear { stops, .. } => stops,
        }
    }

    /// The perceptual average (OKLab) of the stops, for places that need one colour.
    pub fn average(&self) -> Rgba {
        let colors = self.colors();
        let n = colors.len().max(1) as f32;
        let (mut l, mut a, mut b, mut alpha) = (0.0, 0.0, 0.0, 0.0);
        for c in colors {
            let lab = c.to_oklab();
            l += lab.l;
            a += lab.a;
            b += lab.b;
            alpha += c.a;
        }
        Oklab {
            l: l / n,
            a: a / n,
            b: b / n,
        }
        .to_oklch()
        .to_rgba(alpha / n)
    }

    fn map(self, f: impl Fn(Rgba) -> Rgba) -> Self {
        match self {
            Self::Solid(c) => Self::Solid(f(c)),
            Self::Linear { angle, stops } => Self::Linear {
                angle,
                stops: stops.into_iter().map(f).collect(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Paint(Paint),
    Shadow(Shadow),
    Number(f32),
}

/// Inputs that do not come from the theme file.
#[derive(Clone, Debug, Default)]
pub struct ResolveOptions {
    /// Accent seed from Windows, the wallpaper or a custom pick; `None` = the theme's own
    /// accent with its `[palette.pin]` values.
    pub accent: Option<Rgba>,
    /// Solid material (Windows 10, transparency off, battery saver): translucent backgrounds
    /// are composited onto the theme's solid base colour.
    pub solid: bool,
    /// User overrides applied after the theme (`theme-overrides.toml`, Theme Studio).
    pub overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct ResolvedTheme {
    pub id: String,
    pub mode: Mode,
    pub palette: Palette,
    pub fonts: Fonts,
    pub material: Material,
    pub warnings: Vec<ThemeWarning>,
    values: Vec<Value>,
}

impl ResolvedTheme {
    pub fn value(&self, token: Token) -> &Value {
        &self.values[token as usize]
    }

    /// A paint or colour token (transparent for other kinds).
    pub fn paint(&self, token: Token) -> &Paint {
        match self.value(token) {
            Value::Paint(p) => p,
            _ => &NO_PAINT,
        }
    }

    /// The colour of a token; gradients give their average (THEMING.md §14).
    pub fn color(&self, token: Token) -> Rgba {
        match self.value(token) {
            Value::Paint(Paint::Solid(c)) => *c,
            Value::Paint(p) => p.average(),
            Value::Shadow(s) => s.color,
            Value::Number(_) => TRANSPARENT,
        }
    }

    pub fn number(&self, token: Token) -> f32 {
        match self.value(token) {
            Value::Number(n) => *n,
            _ => 0.0,
        }
    }

    pub fn shadow(&self, token: Token) -> Option<Shadow> {
        match self.value(token) {
            Value::Shadow(s) => Some(*s),
            _ => None,
        }
    }
}

/// Background and surface tokens are what solid material makes opaque. The scrim stays a
/// translucent dimming layer.
fn is_background(id: &str) -> bool {
    (id.starts_with("bg.") || id.starts_with("surface.")) && id != "scrim"
}

pub fn resolve(
    file: &ThemeFile,
    mode: Mode,
    options: &ResolveOptions,
) -> Result<ResolvedTheme, ThemeError> {
    let seed = |s: &str| Rgba::from_hex(s).ok_or_else(|| ThemeError::BadSeed(s.to_string()));
    let accent = options
        .accent
        .map_or_else(|| seed(&file.palette.accent), Ok)?;
    let mut palette = Palette::generate(accent, seed(&file.palette.neutral)?);
    let mut warnings = Vec::new();
    if options.accent.is_none() {
        for (key, hex) in &file.palette.pin {
            match Rgba::from_hex(hex) {
                Some(c) if palette.pin(key, c) => {}
                _ => warnings.push(ThemeWarning {
                    key: format!("palette.pin.{key}"),
                    message: format!("ignored pin `{hex}`"),
                }),
            }
        }
    }
    let solid_base = match mode {
        Mode::Light => &file.material.solid_light,
        Mode::Dark => &file.material.solid_dark,
    };
    let solid_base =
        Rgba::from_hex(solid_base).ok_or_else(|| ThemeError::BadSeed(solid_base.clone()))?;

    // Layers, lowest first: numbers → [common] → [light]/[dark] → user overrides.
    let mut layers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, n) in &file.numbers {
        layers.entry(key.clone()).or_default().push(n.to_string());
    }
    let variant = match mode {
        Mode::Light => &file.light,
        Mode::Dark => &file.dark,
    };
    for layer in [&file.common, variant, &options.overrides] {
        for (key, value) in layer {
            layers.entry(key.clone()).or_default().push(value.clone());
        }
    }
    for key in layers.keys() {
        if Token::from_id(key).is_none() {
            warnings.push(ThemeWarning {
                key: key.clone(),
                message: format!("unknown token{}", suggestion(key)),
            });
        }
    }

    let mut resolver = Resolver {
        layers: &layers,
        palette: &palette,
        mode,
        solid: options.solid.then_some(solid_base),
        memo: BTreeMap::new(),
        stack: Vec::new(),
        warnings: BTreeMap::new(),
    };
    let values: Vec<Value> = TOKENS
        .iter()
        .map(|meta| {
            resolver.token(meta.id).unwrap_or_else(|message| {
                resolver.warn(meta.id, message);
                fallback(meta.ty)
            })
        })
        .collect();
    warnings.extend(
        resolver
            .warnings
            .into_iter()
            .map(|(key, messages)| ThemeWarning {
                key,
                message: messages.into_iter().collect::<Vec<_>>().join("; "),
            }),
    );

    // Palette entries are not tokens; the pinned palette is kept for the bridge and Theme Studio.
    Ok(ResolvedTheme {
        id: file.meta.id.clone(),
        mode,
        palette,
        fonts: file.fonts.clone(),
        material: file.material.clone(),
        warnings,
        values,
    })
}

struct Resolver<'a> {
    layers: &'a BTreeMap<String, Vec<String>>,
    palette: &'a Palette,
    mode: Mode,
    /// Base colour for solid material, when it is active.
    solid: Option<Rgba>,
    /// Final value per token, or the error once every layer failed.
    memo: BTreeMap<String, Result<Value, String>>,
    stack: Vec<String>,
    warnings: BTreeMap<String, BTreeSet<String>>,
}

impl Resolver<'_> {
    fn warn(&mut self, key: &str, message: String) {
        self.warnings
            .entry(key.to_string())
            .or_default()
            .insert(message);
    }

    fn token(&mut self, id: &str) -> Result<Value, String> {
        if let Some(color) = self.palette.get(id) {
            return Ok(Value::Paint(Paint::Solid(color)));
        }
        if let Some(result) = self.memo.get(id) {
            return result.clone();
        }
        if let Some(start) = self.stack.iter().position(|s| s == id) {
            let mut chain = self.stack[start..].to_vec();
            chain.push(id.to_string());
            return Err(format!("reference cycle: {}", chain.join(" → ")));
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(format!("references nest deeper than {MAX_DEPTH}"));
        }
        let Some(meta) = Token::from_id(id).map(Token::meta) else {
            return Err(format!("unknown reference `{{{id}}}`{}", suggestion(id)));
        };
        let Some(sources) = self.layers.get(id) else {
            return Err("no value in this theme".into());
        };
        // Highest layer first; a failing layer falls back to the one below it.
        self.stack.push(id.to_string());
        let mut result = Err(String::new());
        for (depth, source) in sources.iter().enumerate().rev() {
            let attempt = value::parse(source)
                .map_err(|e| format!("`{source}`: {e}"))
                .and_then(|expr| self.eval(&expr))
                .and_then(|v| coerce(meta.ty, v));
            match attempt {
                Ok(value) => {
                    result = Ok(value);
                    break;
                }
                Err(message) => {
                    let fallback = if depth > 0 {
                        "; using the value below it"
                    } else {
                        ""
                    };
                    self.warn(id, format!("{message}{fallback}"));
                    result = Err(format!("depends on `{id}`, which has no valid value"));
                }
            }
        }
        self.stack.pop();
        let result = result.map(|value| self.environment(id, value));
        self.memo.insert(id.to_string(), result.clone());
        result
    }

    /// Solid material: backgrounds become opaque where they are defined, so every token that
    /// refers to them inherits the opaque colour.
    fn environment(&self, id: &str, value: Value) -> Value {
        match (self.solid, value) {
            (Some(base), Value::Paint(paint)) if is_background(id) => {
                Value::Paint(paint.map(|c| c.over(base)))
            }
            (_, value) => value,
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, String> {
        Ok(match expr {
            Expr::Color(c) => Value::Paint(Paint::Solid(*c)),
            Expr::Transparent => Value::Paint(Paint::Solid(TRANSPARENT)),
            Expr::Number(n) => Value::Number(*n),
            Expr::Ref(id) => self.token(id)?,
            Expr::Gradient { angle, stops } => Value::Paint(Paint::Linear {
                angle: *angle,
                stops: stops
                    .iter()
                    .map(|stop| self.color(stop))
                    .collect::<Result<_, _>>()?,
            }),
            Expr::Shadow {
                x,
                y,
                blur,
                spread,
                color,
            } => Value::Shadow(Shadow {
                x: *x,
                y: *y,
                blur: *blur,
                spread: *spread,
                color: self.color(color)?,
            }),
            Expr::Func(func, args) => self.func(*func, args)?,
        })
    }

    /// A single colour (gradients are not accepted here).
    fn color(&mut self, expr: &Expr) -> Result<Rgba, String> {
        match self.eval(expr)? {
            Value::Paint(Paint::Solid(c)) => Ok(c),
            Value::Paint(Paint::Linear { .. }) => Err("a gradient cannot be used here".into()),
            Value::Shadow(_) | Value::Number(_) => Err("expected a colour".into()),
        }
    }

    fn number(&mut self, expr: &Expr) -> Result<f32, String> {
        match self.eval(expr)? {
            Value::Number(n) => Ok(n),
            _ => Err("expected a number".into()),
        }
    }

    fn func(&mut self, func: Func, args: &[Expr]) -> Result<Value, String> {
        let Value::Paint(input) = self.eval(&args[0])? else {
            return Err("expected a colour or gradient".into());
        };
        if func == Func::On {
            // Ink or white, whichever reads better on every stop over this mode's backdrops.
            let backdrops = backdrops(self.mode);
            let worst = |text: Rgba| {
                input
                    .colors()
                    .iter()
                    .flat_map(|bg| backdrops.iter().map(move |b| bg.over(*b)))
                    .map(|surface| contrast(text, surface))
                    .fold(f32::INFINITY, f32::min)
            };
            let pick = if worst(INK) >= worst(WHITE) {
                INK
            } else {
                WHITE
            };
            return Ok(Value::Paint(Paint::Solid(pick)));
        }
        if func == Func::Mix {
            let Paint::Solid(a) = input else {
                return Err("mix() takes colours, not gradients".into());
            };
            let b = self.color(&args[1])?;
            let percent = self.number(&args[2])?;
            if !(0.0..=100.0).contains(&percent) {
                return Err(format!("mix() needs 0%–100%, got {percent}%"));
            }
            return Ok(Value::Paint(Paint::Solid(color::mix(
                a,
                b,
                percent / 100.0,
            ))));
        }
        let n = self.number(&args[1])?;
        if func == Func::Alpha && !(0.0..=1.0).contains(&n) {
            return Err(format!("alpha() needs 0–1, got {n}"));
        }
        Ok(Value::Paint(input.map(|c| {
            let lch = c.to_oklch();
            let adjusted = |lch: Oklch| lch.to_rgba(c.a);
            match func {
                Func::Alpha => c.with_alpha(n),
                Func::Lighten => adjusted(Oklch {
                    l: lch.l + n,
                    ..lch
                }),
                Func::Darken => adjusted(Oklch {
                    l: lch.l - n,
                    ..lch
                }),
                Func::Saturate => adjusted(Oklch {
                    c: lch.c + n,
                    ..lch
                }),
                Func::Desaturate => adjusted(Oklch {
                    c: (lch.c - n).max(0.0),
                    ..lch
                }),
                Func::ShiftHue => adjusted(Oklch {
                    h: (lch.h + n).rem_euclid(360.0),
                    ..lch
                }),
                Func::On | Func::Mix => c,
            }
        })))
    }
}

/// Type check of a resolved value against its token (THEMING.md §4: gradients only in Paint).
fn coerce(ty: TokenType, value: Value) -> Result<Value, String> {
    match (ty, value) {
        (TokenType::Paint, v @ Value::Paint(_))
        | (TokenType::Color, v @ Value::Paint(Paint::Solid(_)))
        | (TokenType::Shadow, v @ Value::Shadow(_))
        | (TokenType::Number, v @ Value::Number(_)) => Ok(v),
        (TokenType::Color, Value::Paint(Paint::Linear { .. })) => {
            Err("a gradient cannot be used for a colour token".into())
        }
        (ty, _) => Err(format!("expected a {ty:?} value")),
    }
}

fn fallback(ty: TokenType) -> Value {
    match ty {
        TokenType::Color | TokenType::Paint => Value::Paint(Paint::Solid(TRANSPARENT)),
        TokenType::Shadow => Value::Shadow(Shadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: TRANSPARENT,
        }),
        TokenType::Number => Value::Number(0.0),
    }
}

/// ", did you mean `…`?" for the closest token id, if it is close enough.
fn suggestion(unknown: &str) -> String {
    TOKENS
        .iter()
        .map(|m| (edit_distance(unknown, m.id), m.id))
        .min()
        .filter(|(d, _)| *d <= 3)
        .map(|(_, id)| format!(", did you mean `{id}`?"))
        .unwrap_or_default()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            row.push((prev[j] + cost).min(prev[j + 1] + 1).min(row[j] + 1));
        }
        prev = row;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cerulean() -> ThemeFile {
        ThemeFile::parse(crate::builtin::CERULEAN).unwrap()
    }

    fn warning_for<'a>(t: &'a ResolvedTheme, key: &str) -> Option<&'a ThemeWarning> {
        t.warnings.iter().find(|w| w.key == key)
    }

    fn with_overrides(pairs: &[(&str, &str)]) -> ResolveOptions {
        ResolveOptions {
            overrides: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            ..ResolveOptions::default()
        }
    }

    #[test]
    fn bad_override_falls_back_to_the_theme_value() {
        let base = resolve(&cerulean(), Mode::Light, &ResolveOptions::default()).unwrap();
        let t = resolve(
            &cerulean(),
            Mode::Light,
            &with_overrides(&[("fg.primary", "#12")]),
        )
        .unwrap();
        assert_eq!(t.color(Token::FgPrimary), base.color(Token::FgPrimary));
        // Tokens built on it are untouched and get no misleading warnings.
        assert_eq!(t.color(Token::OsdFg), base.color(Token::OsdFg));
        assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
        assert!(warning_for(&t, "fg.primary").is_some());
    }

    #[test]
    fn cycles_are_reported_with_their_chain() {
        let mut file = cerulean();
        file.dark.insert("hover.bg".into(), "{pressed.bg}".into());
        file.dark.insert("pressed.bg".into(), "{hover.bg}".into());
        let t = resolve(&file, Mode::Dark, &ResolveOptions::default()).unwrap();
        // The token that closes the loop reports the chain; the other one says what it waits on.
        assert!(
            t.warnings
                .iter()
                .any(|w| w.message.contains("hover.bg → pressed.bg → hover.bg")),
            "{:?}",
            t.warnings
        );
        assert!(warning_for(&t, "hover.bg").is_some());
    }

    #[test]
    fn gradient_in_colour_token_is_rejected() {
        let t = resolve(
            &cerulean(),
            Mode::Light,
            &with_overrides(&[("fg.link", "gradient(90deg, #000000, #FFFFFF)")]),
        )
        .unwrap();
        assert!(warning_for(&t, "fg.link").is_some());
        assert_ne!(
            t.color(Token::FgLink),
            TRANSPARENT,
            "falls back to the theme value"
        );
    }

    #[test]
    fn unknown_tokens_get_suggestions() {
        let t = resolve(
            &cerulean(),
            Mode::Light,
            &with_overrides(&[("buton.primary.fg", "#FFFFFF")]),
        )
        .unwrap();
        let w = warning_for(&t, "buton.primary.fg").unwrap();
        assert!(w.message.contains("button.primary.fg"), "{w}");
    }

    #[test]
    fn overrides_win_and_custom_accent_skips_pins() {
        let mut options = with_overrides(&[("button.primary.bg_hover", "#FF0000")]);
        options.accent = Some(Rgba::from_hex("#7C3AED").unwrap());
        let t = resolve(&cerulean(), Mode::Light, &options).unwrap();
        assert_eq!(t.color(Token::ButtonPrimaryBgHover).to_hex(), "#FF0000");
        assert_ne!(t.palette.accent[5].to_hex(), "#2FA8FF");
    }

    #[test]
    fn solid_material_reaches_derived_tokens() {
        let options = ResolveOptions {
            solid: true,
            ..ResolveOptions::default()
        };
        let t = resolve(&cerulean(), Mode::Dark, &options).unwrap();
        for token in [
            Token::BgWindow,
            Token::SurfaceOverlay,
            Token::OsdBg,
            Token::ToastBg,
            Token::TabActiveBg,
        ] {
            assert!(
                (t.color(token).a - 1.0).abs() < 1e-6,
                "{} is not opaque",
                token.id()
            );
        }
        assert!(
            t.color(Token::SelectionBg).a < 1.0,
            "state colours stay translucent"
        );
        assert!(t.color(Token::Scrim).a < 1.0, "the scrim stays translucent");
    }

    #[test]
    fn mix_and_alpha_ranges_are_checked() {
        let t = resolve(
            &cerulean(),
            Mode::Light,
            &with_overrides(&[
                ("hover.bg", "alpha(#000000, 2)"),
                ("pressed.bg", "mix(#000000, #FFFFFF, 150%)"),
            ]),
        )
        .unwrap();
        assert!(warning_for(&t, "hover.bg").is_some());
        assert!(warning_for(&t, "pressed.bg").is_some());
    }

    #[test]
    fn warnings_are_deterministic() {
        let options = with_overrides(&[("zz.unknown", "#000000"), ("aa.unknown", "#000000")]);
        let a = resolve(&cerulean(), Mode::Light, &options)
            .unwrap()
            .warnings;
        let b = resolve(&cerulean(), Mode::Light, &options)
            .unwrap()
            .warnings;
        assert_eq!(a, b);
        assert_eq!(a[0].key, "aa.unknown");
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
