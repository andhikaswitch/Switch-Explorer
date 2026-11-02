//! File operation engine: plans with conflict decisions, a cancellable executor, the
//! write-ahead journal and undo/redo (FEATURES.md F-OPS, `.claude/rules/file-ops-safety.md`).
//!
//! Safety rules enforced here, not only in the UI:
//! - nothing is overwritten: Replace recycles the old item first;
//! - delete goes to the Recycle Bin unless a permanent delete was confirmed by the caller;
//! - every operation is journaled before and after it runs, and can be undone;
//! - cancel is honoured between items and between files inside folders.
#![forbid(unsafe_code)]

pub mod error;
pub mod exec;
pub mod journal;
pub mod name;
pub mod op;
pub mod plan;
pub mod undo;

pub use error::OpsError;
pub use exec::{Cancel, Outcome, Progress, Trash, run};
pub use journal::Journal;
pub use op::{Op, Pair};
pub use plan::{Conflict, Mode, Resolution};
pub use undo::UndoStack;
