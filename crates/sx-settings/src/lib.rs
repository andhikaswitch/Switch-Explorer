//! Settings schema, feature flags and Hot Switch metadata, presets, keymap, TOML persistence.
//!
//! "Every feature is a switch": each behaviour is a flag declared once in [`flags`], with
//! metadata that the Settings page, the flag page, the Hot Switch panel and the keymap editor
//! are generated from (DEVELOPER-MODE.md).
#![forbid(unsafe_code)]

pub mod error;
pub mod flags;
pub mod hotswitch;
pub mod meta;
pub mod store;
pub mod value;

pub use error::SettingsError;
pub use flags::{FLAGS, Features, meta};
pub use hotswitch::{DEFAULT_BINDINGS, HotSwitchAction, HotSwitchBinding, PRESETS, Preset};
pub use meta::{FlagMeta, Group, Scope, Stability, Visibility};
pub use store::{Change, PendingWrite, SettingsStore};
pub use value::{FlagKind, FlagValue};
