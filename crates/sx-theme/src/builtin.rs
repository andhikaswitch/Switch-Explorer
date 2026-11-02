//! Built-in themes, compiled into the binary. `themes/*.toml` at the repository root is the
//! source of truth for their values (CLAUDE.md, repository layout).

pub const CERULEAN: &str = include_str!("../../../themes/cerulean.toml");

/// `(id, source)` of every built-in theme.
pub const BUILTIN: &[(&str, &str)] = &[("cerulean", CERULEAN)];

pub fn source(id: &str) -> Option<&'static str> {
    BUILTIN.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
}
