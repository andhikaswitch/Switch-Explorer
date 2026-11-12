//! Undo/redo stack of completed operations (FEATURES.md F-OPS-07, `ops.undo_depth`).
//!
//! Both sides hold what operations actually did, never what they planned. Undo runs the inverse
//! of the last operation; once it finishes, the inverse of what the undo did becomes the redo.
//! So an item that undo restored as "name (2)" is the one redo recycles again, and a recycled
//! item comes back from its own slot. Redo runs that, and what it did goes back to the undo side.

use crate::op::Op;

#[derive(Debug, Default)]
pub struct UndoStack {
    undo: Vec<Op>,
    redo: Vec<Op>,
    depth: usize,
    /// Bumped by every user operation: an undo or redo that finishes after one does not write
    /// into the history that operation started.
    generation: u64,
}

impl UndoStack {
    pub fn new(depth: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            depth: depth.max(1),
            generation: 0,
        }
    }

    fn push_undo(&mut self, done: Op) -> bool {
        if done.inverse().is_none() || done.is_noop() {
            return false;
        }
        if self.undo.len() == self.depth {
            self.undo.remove(0);
        }
        self.undo.push(done);
        true
    }

    /// Records what a user operation did; clears redo. Operations without an inverse
    /// (permanent delete) and operations that did nothing are not recorded.
    pub fn record(&mut self, done: Op) {
        if self.push_undo(done) {
            self.redo.clear();
            self.generation += 1;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The operation that undoes the last one, without taking it: take it with
    /// [`Self::take_undo`] once its job has started, so an undo that cannot start keeps it.
    pub fn next_undo(&self) -> Option<Op> {
        self.undo.last()?.inverse()
    }

    /// Takes the entry [`Self::next_undo`] returned; the generation goes to [`Self::undone`].
    pub fn take_undo(&mut self) -> u64 {
        self.undo.pop();
        self.generation
    }

    /// What an undo did: its inverse becomes the next redo (unless a newer operation started
    /// a new history meanwhile).
    pub fn undone(&mut self, done: &Op, generation: u64) {
        if generation != self.generation {
            return;
        }
        if let Some(redo) = done.inverse().filter(|redo| !redo.is_noop()) {
            self.redo.push(redo);
        }
    }

    /// What the last operations did, newest first: the first one is what undo takes back.
    pub fn recent(&self, count: usize) -> impl Iterator<Item = &Op> {
        self.undo.iter().rev().take(count)
    }

    /// The operation to run again, without taking it (see [`Self::next_undo`]).
    pub fn next_redo(&self) -> Option<Op> {
        self.redo.last().cloned()
    }

    /// Takes the entry [`Self::next_redo`] returned; the generation goes to [`Self::redone`].
    pub fn take_redo(&mut self) -> u64 {
        self.redo.pop();
        self.generation
    }

    /// What a redo did: it can be undone again (redo keeps the rest of its history).
    pub fn redone(&mut self, done: Op, generation: u64) {
        if generation == self.generation {
            self.push_undo(done);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::op::Binned;

    fn path(n: u32) -> PathBuf {
        PathBuf::from(format!(r"C:\{n}"))
    }

    fn folder(n: u32) -> Op {
        Op::NewFolder { path: path(n) }
    }

    #[test]
    fn recent_lists_newest_first_and_follows_undo() {
        let mut stack = UndoStack::new(10);
        assert_eq!(stack.recent(5).count(), 0);
        for n in 1..=4 {
            stack.record(folder(n));
        }
        let listed: Vec<&Op> = stack.recent(3).collect();
        assert_eq!(listed, [&folder(4), &folder(3), &folder(2)]);
        // What undo takes back leaves the list: its first entry is always the next undo.
        stack.take_undo();
        assert_eq!(stack.recent(1).next(), Some(&folder(3)));
    }

    #[test]
    fn undo_redo_and_depth() {
        let mut stack = UndoStack::new(2);
        stack.record(folder(1));
        stack.record(folder(2));
        stack.record(folder(3));
        let undo = stack.next_undo();
        assert!(matches!(&undo, Some(Op::Recycle { paths, .. }) if paths[0] == path(3)));
        let generation = stack.take_undo();
        // The undo ran and put the folder in slot $R3.
        let slot = Binned {
            original: path(3),
            slot: PathBuf::from(r"C:\$Recycle.Bin\S-1\$R3"),
        };
        stack.undone(
            &Op::Recycle {
                paths: vec![path(3)],
                binned: vec![slot.clone()],
            },
            generation,
        );
        // Redo brings that very item back from its slot.
        assert_eq!(
            stack.next_redo(),
            Some(Op::Restore {
                items: vec![slot.clone()],
                restored: vec![],
            })
        );
        let generation = stack.take_redo();
        stack.redone(
            Op::Restore {
                items: vec![slot],
                restored: vec![path(3)],
            },
            generation,
        );
        assert!(
            matches!(stack.next_undo(), Some(Op::Recycle { paths, .. }) if paths[0] == path(3))
        );
        stack.take_undo();
        assert!(stack.next_undo().is_some());
        stack.take_undo();
        assert!(stack.next_undo().is_none(), "depth 2 forgot the first one");
    }

    #[test]
    fn nothing_done_is_not_recorded() {
        let mut stack = UndoStack::new(4);
        stack.record(folder(1));
        let generation = stack.take_undo();
        stack.undone(&folder(1), generation);
        assert!(stack.can_redo());
        stack.record(Op::Delete { paths: vec![] });
        stack.record(Op::nothing());
        assert!(
            stack.can_redo(),
            "permanent deletes and no-ops are not recorded"
        );
        // An undo that failed at once, or one whose items left no slot, leaves nothing to redo.
        let mut stack = UndoStack::new(4);
        stack.record(folder(2));
        let generation = stack.take_undo();
        stack.undone(&Op::nothing(), generation);
        stack.undone(
            &Op::Recycle {
                paths: vec![path(2)],
                binned: vec![],
            },
            generation,
        );
        assert!(!stack.can_undo() && !stack.can_redo());
    }

    #[test]
    fn a_late_undo_does_not_rewrite_newer_history() {
        let mut stack = UndoStack::new(4);
        stack.record(folder(1));
        // An undo starts; the user does something else before it finishes.
        assert!(stack.next_undo().is_some());
        let generation = stack.take_undo();
        stack.record(folder(2));
        stack.undone(&folder(1), generation);
        assert!(!stack.can_redo(), "the new operation started a new history");
        assert!(stack.can_undo());
    }
}
