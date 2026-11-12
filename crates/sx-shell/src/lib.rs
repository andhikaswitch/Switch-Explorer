//! Shell COM services: icons, thumbnails, context menu, OLE drag and drop, clipboard, DWM, OS theme.
//!
//! Platform crate: `unsafe` is allowed here, with a `// SAFETY:` comment on every block
//! (docs/CODING-STANDARDS.md §5).

pub mod appearance;
pub mod clipboard;
pub mod com;
pub mod dnd;
pub mod dwm;
pub mod error;
pub mod exec;
pub mod icons;
pub mod menu;
pub mod modal;
pub mod names;
pub mod recent;
pub mod recycle;
pub mod taskbar;
pub mod worker;

pub use appearance::OsAppearance;
pub use com::StaGuard;
pub use error::ShellError;
pub use worker::{Order, StaWorker};
