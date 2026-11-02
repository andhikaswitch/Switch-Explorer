//! Resolution pipeline (THEMING.md §8): palette → `[common]` → `[light]`/`[dark]` → topological
//! evaluation with cycle detection → environment adjustments. The result is a flat array
//! indexed by [`Token`], so lookups at render time cost nothing.

use std::collections::{BTreeMap, HashMap};

use crate::color::{self, INK, Oklch, Rgba, TRANSPARENT, WHITE, contrast};
use crate::error::{ThemeError, ThemeWarning};
use crate::file::{Fonts, Material, ThemeFile};
use crate::palette::Palette;
use crate::schema::{TOKENS, Token, TokenType};
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

impl Paint {
    /// Every colour the paint can show (the stops of a gradient).
    pub fn colors(&self) -> &[Rgba] {
        match self {
            Self::Solid(c) => std::slice::from_ref(c),
            Self::Linear { stops, .. } => stops,
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
    /// are composited onto the variant's base colour.
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

    /// A paint token, or a colour token as a solid paint.
    pub fn paint(&self, token: Token) -> Paint {
        match self.value(token) {
            Value::Paint(p) => p.clone(),
            _ => Paint::Solid(TRANSPARENT),
        }
    }

    /// The colour of a token; for gradients, the first stop.
    pub fn color(&self, token: Token) -> Rgba {
        match self.value(token) {
            Value::Paint(p) => p.colors().first().copied().unwrap_or(TRANSPARENT),
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

/// Base colour under translucent backgrounds in solid mode (the Mica fallback colours).
pub fn solid_base(mode: Mode) -> Rgba {
    match mode {
        Mode::Light => Rgba::rgb(0xF3, 0xF3, 0xF3),
        Mode::Dark => Rgba::rgb(0x20, 0x20, 0x20),
    }
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

    // Layers: numbers → [common] → [light]/[dark] → user overrides (later wins).
    let mut sources: HashMap<String, String> = HashMap::new();
    for (key, n) in &file.numbers {
        sources.insert(key.clone(), n.to_string());
    }
    let variant = match mode {
        Mode::Light => &file.light,
        Mode::Dark => &file.dark,
    };
    for layer in [&file.common, variant, &options.overrides] {
        for (key, value) in layer {
            sources.insert(key.clone(), value.clone());
        }
    }
    for key in sources.keys() {
        if Token::from_id(key).is_none() {
            warnings.push(ThemeWarning {
                key: key.clone(),
                message: format!("unknown token{}", suggestion(key)),
            });
        }
    }

    let mut resolver = Resolver {
        sources: &sources,
        palette: &palette,
        memo: HashMap::new(),
        stack: Vec::new(),
    };
    let mut values = Vec::with_capacity(TOKENS.len());
    for meta in TOKENS {
        let value = match resolver.token(meta.id) {
            Ok(value) => coerce(meta.ty, value).unwrap_or_else(|message| {
                warnings.push(ThemeWarning {
                    key: meta.id.into(),
                    message,
                });
                fallback(meta.ty)
            }),
            Err(message) => {
                warnings.push(ThemeWarning {
                    key: meta.id.into(),
                    message,
                });
                fallback(meta.ty)
            }
        };
        values.push(value);
    }

    if options.solid {
        let base = solid_base(mode);
        for meta in TOKENS {
            let solidify = meta.id.starts_with("bg.") || meta.id.starts_with("surface.");
            if solidify && let Value::Paint(paint) = &mut values[meta.token as usize] {
                match paint {
                    Paint::Solid(c) => *c = c.over(base),
                    Paint::Linear { stops, .. } => {
                        for c in stops.iter_mut() {
                            *c = c.over(base);
                        }
                    }
                }
            }
        }
    }

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
    sources: &'a HashMap<String, String>,
    palette: &'a Palette,
    memo: HashMap<String, Value>,
    stack: Vec<String>,
}

impl Resolver<'_> {
    fn token(&mut self, id: &str) -> Result<Value, String> {
        if let Some(color) = self.palette.get(id) {
            return Ok(Value::Paint(Paint::Solid(color)));
        }
        if let Some(value) = self.memo.get(id) {
            return Ok(value.clone());
        }
        if let Some(start) = self.stack.iter().position(|s| s == id) {
            let mut chain = self.stack[start..].to_vec();
            chain.push(id.to_string());
            return Err(format!("reference cycle: {}", chain.join(" → ")));
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(format!("references nest deeper than {MAX_DEPTH}"));
        }
        let Some(source) = self.sources.get(id) else {
            return Err(if Token::from_id(id).is_some() {
                "no value in this theme".to_string()
            } else {
                format!("unknown reference `{{{id}}}`{}", suggestion(id))
            });
        };
        let expr = value::parse(source).map_err(|e| format!("`{source}`: {e}"))?;
        self.stack.push(id.to_string());
        let result = self.eval(&expr);
        self.stack.pop();
        let value = result?;
        self.memo.insert(id.to_string(), value.clone());
        Ok(value)
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
            // Ink or white, whichever reads better on every stop.
            let worst = |text: Rgba| {
                input
                    .colors()
                    .iter()
                    .map(|bg| contrast(text, bg.over(WHITE)))
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
            let t = self.number(&args[2])? / 100.0;
            return Ok(Value::Paint(Paint::Solid(color::mix(a, b, t))));
        }
        let n = self.number(&args[1])?;
        let apply = |c: Rgba| -> Rgba {
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
        };
        Ok(Value::Paint(match input {
            Paint::Solid(c) => Paint::Solid(apply(c)),
            Paint::Linear { angle, stops } => Paint::Linear {
                angle,
                stops: stops.into_iter().map(apply).collect(),
            },
        }))
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

    fn theme(common: &[(&str, &str)]) -> ThemeFile {
        let mut file = ThemeFile::parse(crate::builtin::CERULEAN).unwrap();
        for (k, v) in common {
            file.common.insert((*k).into(), (*v).into());
        }
        file
    }

    fn warning_for<'a>(t: &'a ResolvedTheme, key: &str) -> Option<&'a ThemeWarning> {
        t.warnings.iter().find(|w| w.key == key)
    }

    #[test]
    fn cycles_are_reported_with_their_chain() {
        let mut file = theme(&[]);
        file.dark.insert("hover.bg".into(), "{pressed.bg}".into());
        file.dark.insert("pressed.bg".into(), "{hover.bg}".into());
        let t = resolve(&file, Mode::Dark, &ResolveOptions::default()).unwrap();
        let w = warning_for(&t, "hover.bg").unwrap();
        assert!(w.message.contains("cycle"), "{w}");
        assert!(
            w.message.contains("hover.bg → pressed.bg → hover.bg"),
            "{w}"
        );
    }

    #[test]
    fn gradient_in_colour_token_is_rejected() {
        let file = theme(&[("fg.link", "gradient(90deg, #000000, #FFFFFF)")]);
        let mut file = file;
        file.light.remove("fg.link");
        let t = resolve(&file, Mode::Light, &ResolveOptions::default()).unwrap();
        assert!(warning_for(&t, "fg.link").is_some());
        assert_eq!(t.color(Token::FgLink), TRANSPARENT);
    }

    #[test]
    fn unknown_tokens_get_suggestions() {
        let file = theme(&[("buton.primary.fg", "#FFFFFF")]);
        let t = resolve(&file, Mode::Light, &ResolveOptions::default()).unwrap();
        let w = warning_for(&t, "buton.primary.fg").unwrap();
        assert!(w.message.contains("button.primary.fg"), "{w}");
    }

    #[test]
    fn overrides_win_and_custom_accent_skips_pins() {
        let file = theme(&[]);
        let mut options = ResolveOptions::default();
        options
            .overrides
            .insert("button.primary.bg_hover".into(), "#FF0000".into());
        options.accent = Some(Rgba::from_hex("#7C3AED").unwrap());
        let t = resolve(&file, Mode::Light, &options).unwrap();
        assert_eq!(t.color(Token::ButtonPrimaryBgHover).to_hex(), "#FF0000");
        assert_ne!(t.palette.accent[5].to_hex(), "#2FA8FF");
    }

    #[test]
    fn solid_material_makes_backgrounds_opaque() {
        let file = theme(&[]);
        let options = ResolveOptions {
            solid: true,
            ..ResolveOptions::default()
        };
        let t = resolve(&file, Mode::Dark, &options).unwrap();
        assert!((t.color(Token::BgWindow).a - 1.0).abs() < 1e-6);
        assert!(
            t.color(Token::SelectionBg).a < 1.0,
            "only bg/surface are made opaque"
        );
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
