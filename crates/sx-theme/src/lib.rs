//! Theme tokens, value parser, OKLCH colour, palette generator, resolver and contrast validator.
//!
//! Three layers (THEMING.md §2): the palette (`pal.*`, generated from seeds) is referenced by
//! semantic tokens, which are referenced by component tokens. Theme files only give values;
//! every token is declared once in [`schema`].
#![forbid(unsafe_code)]

pub mod builtin;
pub mod color;
pub mod error;
pub mod file;
pub mod palette;
pub mod resolve;
pub mod schema;
pub mod validate;
pub mod value;

pub use color::{Oklch, Rgba};
pub use error::{ThemeError, ThemeWarning};
pub use file::ThemeFile;
pub use palette::Palette;
pub use resolve::{Mode, Paint, ResolveOptions, ResolvedTheme, Shadow, Value, resolve};
pub use schema::{TOKENS, Token, TokenType};
