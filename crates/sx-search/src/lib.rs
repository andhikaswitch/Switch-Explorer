//! Recursive search, fuzzy matching and the Everything adapter (FEATURES.md F-SRCH).
#![forbid(unsafe_code)]

pub mod query;
pub mod walk;

pub use query::{Candidate, KindFilter, Query, QueryError, SizeFilter};
pub use walk::{Hit, Stats, WalkOptions, search};
