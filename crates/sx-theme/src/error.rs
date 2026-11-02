use std::fmt;

use thiserror::Error;

/// Errors that stop a theme from loading at all. Problems with single values are
/// [`ThemeWarning`]s: the token falls back and the rest of the theme still applies.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThemeError {
    #[error("the theme file is {0} bytes; the limit is 1 MB")]
    TooLarge(usize),
    #[error("the theme file is not valid: {0}")]
    Toml(String),
    #[error("invalid palette seed `{0}`: use a colour like #2FA8FF")]
    BadSeed(String),
}

/// A problem with one token value (THEMING.md §11: warnings never block).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeWarning {
    pub key: String,
    pub message: String,
}

impl fmt::Display for ThemeWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.message)
    }
}
