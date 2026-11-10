//! Recursive search, fuzzy matching and the Everything adapter (FEATURES.md F-SRCH).
#![forbid(unsafe_code)]

pub mod fuzzy;
pub mod query;
pub mod walk;

pub use fuzzy::{FuzzyMatch, fuzzy_match};
pub use query::{Candidate, KindFilter, Query, QueryError, SizeFilter};
pub use walk::{Stats, WalkOptions, entry_path, search};
