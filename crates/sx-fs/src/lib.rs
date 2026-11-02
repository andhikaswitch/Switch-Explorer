//! Directory model, natural sort keys, filtering and folder watcher.
#![forbid(unsafe_code)]

pub mod snapshot;

pub use snapshot::{Column, DirSnapshot, EntryView, SortSpec, ViewOptions};
