//! Domain types, actions, events, command registry and errors (no OS dependencies).
#![forbid(unsafe_code)]

pub mod command;
pub mod entry;
pub mod error;
pub mod event;
pub mod location;
pub mod selection;

pub use command::{CommandContext, CommandId, CommandRegistry, CommandSpec};
pub use entry::EntryKind;
pub use error::CoreError;
pub use event::{ChangeSource, SystemAppearance};
pub use location::Location;
pub use selection::Selection;
