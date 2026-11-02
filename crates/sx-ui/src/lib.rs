//! GPUI Kit views and components, theme bridge, OSD and Token Inspector.
#![forbid(unsafe_code)]
// Render helpers are methods on the view for uniform `self.render_x(cx)` call sites, even when a
// helper only needs the context.
#![allow(clippy::unused_self)]

rust_i18n::i18n!("../../locales", fallback = "en");

pub mod actions;
mod chrome;
mod details;
mod format;
mod pages;
mod sidebar;
mod status;
mod tab;
pub mod theme;
pub mod workspace;

pub use theme::{ActiveTokens, ThemeController, Tokens};
pub use workspace::{Workspace, WorkspaceOptions};

/// UI language: `id` for Indonesian, `en` otherwise (FEATURES.md F-I18N-01).
pub fn set_locale(lang: &str) {
    let lang = if lang.eq_ignore_ascii_case("id") {
        "id"
    } else {
        "en"
    };
    rust_i18n::set_locale(lang);
}
