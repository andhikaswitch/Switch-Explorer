//! Safe Win32 wrappers for the file system: enumeration, `CopyFile2`, volumes, long paths.
//!
//! Platform crate: `unsafe` is allowed here, with a `// SAFETY:` comment on every block
//! (docs/CODING-STANDARDS.md §5). Nothing raw leaves this crate: handles are wrapped in RAII
//! types and the public API takes and returns plain Rust values.

pub mod enumerate;
pub mod error;
pub mod format;
pub mod known;
pub mod os;
pub mod path;
pub mod sort;
pub mod volume;

pub use enumerate::{BATCH, RawEntry, enumerate};
pub use error::WinError;
pub use known::{KnownFolder, known_folder};
pub use os::{BUILD_SYSTEM_BACKDROP, os_build};
pub use path::{to_wide, verbatim};
pub use sort::{natural_sort_key, str_cmp_logical};
pub use volume::{Drive, DriveKind, Space, drives};
