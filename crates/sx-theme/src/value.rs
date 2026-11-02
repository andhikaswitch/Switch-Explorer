//! Parser for theme values (THEMING.md §4):
//!
//! ```text
//! value    := color | ref | function | gradient | shadow | number | transparent
//! color    := #RRGGBB | #RRGGBBAA | rgb(r, g, b) | rgba(r, g, b, a) | oklch(L C h [/ a])
//! ref      := {token.id}
//! function := alpha(v, 0..1) | mix(v, v, 0..100%) | lighten(v, n) | darken(v, n)
//!           | saturate(v, n) | desaturate(v, n) | shift_hue(v, deg) | on(v)
//! gradient := gradient(<angle>deg, v, v[, v…])
//! shadow   := shadow(x, y, blur, spread, v)
//! ```

use std::fmt;

use crate::color::{Oklch, Rgba};

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Color(Rgba),
    Ref(String),
    Number(f32),
    Transparent,
    Func(Func, Vec<Expr>),
    Gradient {
        angle: f32,
        stops: Vec<Expr>,
    },
    Shadow {
        x: f32,
        y: f32,
        blur: f32,
        spread: f32,
        color: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Alpha,
    Mix,
    Lighten,
    Darken,
    Saturate,
    Desaturate,
    ShiftHue,
    On,
}

impl Func {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alpha" => Self::Alpha,
            "mix" => Self::Mix,
            "lighten" => Self::Lighten,
            "darken" => Self::Darken,
            "saturate" => Self::Saturate,
            "desaturate" => Self::Desaturate,
            "shift_hue" => Self::ShiftHue,
            "on" => Self::On,
            _ => return None,
        })
    }

    /// Number of arguments (the first is always a colour expression).
    fn arity(self) -> usize {
        match self {
            Self::On => 1,
            Self::Mix => 3,
            _ => 2,
        }
    }
}

/// A parse error with the 1-based column where it was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "column {}: {}", self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(source: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser {
        src: source,
        pos: 0,
    };
    let expr = parser.expr()?;
    parser.skip_ws();
    if parser.pos < source.len() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(expr)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            column: self.src[..self.pos].chars().count() + 1,
            message: message.into(),
        }
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        let trimmed = self.rest().trim_start();
        self.pos = self.src.len() - trimmed.len();
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.rest().starts_with(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), ParseError> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{c}`")))
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &str {
        let start = self.pos;
        let len = self
            .rest()
            .find(|c: char| !f(c))
            .unwrap_or(self.rest().len());
        self.pos += len;
        &self.src[start..start + len]
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        self.skip_ws();
        let Some(first) = self.rest().chars().next() else {
            return Err(self.error("expected a value"));
        };
        match first {
            '#' => self.hex(),
            '{' => self.reference(),
            c if c.is_ascii_digit() || c == '-' || c == '.' => Ok(Expr::Number(self.number()?)),
            c if c.is_ascii_alphabetic() => self.word(),
            _ => Err(self.error(format!("unexpected `{first}`"))),
        }
    }

    fn hex(&mut self) -> Result<Expr, ParseError> {
        let start = self.pos;
        self.pos += 1;
        let digits = self.take_while(|c| c.is_ascii_hexdigit()).to_string();
        Rgba::from_hex(&format!("#{digits}"))
            .map(Expr::Color)
            .ok_or_else(|| {
                self.pos = start;
                self.error("expected #RRGGBB or #RRGGBBAA")
            })
    }

    fn reference(&mut self) -> Result<Expr, ParseError> {
        self.pos += 1;
        let id = self
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .to_string();
        if id.is_empty() {
            return Err(self.error("expected a token id inside `{}`"));
        }
        self.expect('}')?;
        Ok(Expr::Ref(id))
    }

    fn number(&mut self) -> Result<f32, ParseError> {
        self.skip_ws();
        let start = self.pos;
        if self.rest().starts_with('-') {
            self.pos += 1;
        }
        self.take_while(|c| c.is_ascii_digit() || c == '.');
        self.src[start..self.pos].parse().map_err(|_| {
            self.pos = start;
            self.error("expected a number")
        })
    }

    /// A number optionally followed by `unit` (used for `deg`); the unit is not converted.
    fn number_with_unit(&mut self, unit: &str) -> Result<f32, ParseError> {
        let n = self.number()?;
        if self.rest().starts_with(unit) {
            self.pos += unit.len();
        }
        Ok(n)
    }

    fn word(&mut self) -> Result<Expr, ParseError> {
        let start = self.pos;
        let name = self
            .take_while(|c| c.is_ascii_alphanumeric() || c == '_')
            .to_string();
        if name == "transparent" {
            return Ok(Expr::Transparent);
        }
        if !self.eat('(') {
            self.pos = start;
            return Err(self.error(format!("unknown word `{name}`")));
        }
        match name.as_str() {
            "rgb" | "rgba" => self.rgb(name == "rgba"),
            "oklch" => self.oklch(),
            "gradient" => self.gradient(),
            "shadow" => self.shadow(),
            _ => {
                let Some(func) = Func::from_name(&name) else {
                    self.pos = start;
                    return Err(self.error(format!("unknown function `{name}`")));
                };
                self.function(func)
            }
        }
    }

    fn rgb(&mut self, with_alpha: bool) -> Result<Expr, ParseError> {
        let mut channels = [0.0_f32; 4];
        channels[3] = 1.0;
        let count = if with_alpha { 4 } else { 3 };
        for (i, channel) in channels.iter_mut().take(count).enumerate() {
            if i > 0 {
                self.expect(',')?;
            }
            *channel = self.number()?;
        }
        self.expect(')')?;
        let byte = |v: f32| v.clamp(0.0, 255.0) / 255.0;
        Ok(Expr::Color(Rgba {
            r: byte(channels[0]),
            g: byte(channels[1]),
            b: byte(channels[2]),
            a: channels[3].clamp(0.0, 1.0),
        }))
    }

    fn oklch(&mut self) -> Result<Expr, ParseError> {
        let l = self.number()?;
        let c = self.number()?;
        let h = self.number_with_unit("deg")?;
        let a = if self.eat('/') { self.number()? } else { 1.0 };
        self.expect(')')?;
        Ok(Expr::Color(Oklch { l, c, h }.to_rgba(a)))
    }

    fn gradient(&mut self) -> Result<Expr, ParseError> {
        let angle = self.number_with_unit("deg")?;
        let mut stops = Vec::new();
        while self.eat(',') {
            stops.push(self.expr()?);
        }
        self.expect(')')?;
        if stops.len() < 2 {
            return Err(self.error("a gradient needs at least two colours"));
        }
        Ok(Expr::Gradient { angle, stops })
    }

    fn shadow(&mut self) -> Result<Expr, ParseError> {
        let mut n = [0.0_f32; 4];
        for (i, value) in n.iter_mut().enumerate() {
            if i > 0 {
                self.expect(',')?;
            }
            *value = self.number()?;
        }
        self.expect(',')?;
        let color = Box::new(self.expr()?);
        self.expect(')')?;
        Ok(Expr::Shadow {
            x: n[0],
            y: n[1],
            blur: n[2],
            spread: n[3],
            color,
        })
    }

    fn function(&mut self, func: Func) -> Result<Expr, ParseError> {
        let mut args = vec![self.expr()?];
        for _ in 1..func.arity() {
            self.expect(',')?;
            let arg = match (func, args.len()) {
                // mix(a, b, 50%): the second argument is a colour, the third a percentage.
                (Func::Mix, 1) => self.expr()?,
                (Func::Mix, _) => {
                    let n = self.number()?;
                    if !self.rest().starts_with('%') {
                        return Err(self.error("mix() needs a percentage such as 50%"));
                    }
                    self.pos += 1;
                    Expr::Number(n)
                }
                (Func::ShiftHue, _) => Expr::Number(self.number_with_unit("deg")?),
                // alpha/lighten/darken/saturate/desaturate take plain numbers; a unit is an error.
                _ => Expr::Number(self.number()?),
            };
            args.push(arg);
        }
        self.expect(')')?;
        Ok(Expr::Func(func, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_refs() {
        assert!(matches!(parse("#2FA8FF"), Ok(Expr::Color(_))));
        assert_eq!(
            parse(" {pal.accent.500} "),
            Ok(Expr::Ref("pal.accent.500".into()))
        );
        assert_eq!(parse("transparent"), Ok(Expr::Transparent));
        let Ok(Expr::Color(c)) = parse("rgba(255, 0, 0, 0.5)") else {
            panic!("rgba")
        };
        assert!((c.r - 1.0).abs() < 1e-6 && (c.a - 0.5).abs() < 1e-6);
        assert!(matches!(
            parse("oklch(0.7 0.15 240 / 0.8)"),
            Ok(Expr::Color(_))
        ));
    }

    #[test]
    fn nested_functions() {
        let expr =
            parse("gradient(120deg, lighten({pal.lagoon}, 0.04), mix(#000000, #FFFFFF, 50%))")
                .unwrap();
        let Expr::Gradient { angle, stops } = expr else {
            panic!("gradient")
        };
        assert!((angle - 120.0).abs() < 1e-6);
        assert!(matches!(&stops[0], Expr::Func(Func::Lighten, args) if args.len() == 2));
        assert!(matches!(&stops[1], Expr::Func(Func::Mix, args) if args.len() == 3));
    }

    #[test]
    fn shadow_and_on() {
        assert!(matches!(
            parse("shadow(0, 6, 18, 0, alpha({accent.glow}, 0.5))"),
            Ok(Expr::Shadow { blur, .. }) if (blur - 18.0).abs() < 1e-6
        ));
        assert!(matches!(
            parse("on({accent.solid})"),
            Ok(Expr::Func(Func::On, _))
        ));
    }

    #[test]
    fn errors_carry_columns() {
        assert_eq!(parse("#12").unwrap_err().column, 1);
        // Points at the unexpected `0.5`, past the whitespace.
        assert_eq!(parse("alpha(#FFFFFF 0.5)").unwrap_err().column, 15);
        assert!(
            parse("glow(#FFFFFF)")
                .unwrap_err()
                .message
                .contains("unknown function")
        );
        assert!(parse("gradient(90deg, #FFFFFF)").is_err());
        assert!(
            parse("mix(#000000, #FFFFFF, 50)")
                .unwrap_err()
                .message
                .contains('%')
        );
        assert!(parse("alpha(#000000, .5deg)").is_err());
        assert!(parse("shift_hue(#000000, 30deg)").is_ok());
        assert!(parse("#FFFFFF extra").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn never_panics_on_garbage() {
        for input in [
            "",
            "(",
            ")",
            "{",
            "#",
            "rgb(",
            "mix(#000000,",
            "shadow(1,2",
            "–",
            "日本",
        ] {
            let _ = parse(input);
        }
    }
}
