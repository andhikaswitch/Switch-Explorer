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
}

impl UndoStack {
    pub fn new(depth: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            depth: depth.max(1),
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
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The operation that undoes the last one. Report what it did with [`Self::undone`].
    pub fn undo(&mut self) -> Option<Op> {
        self.undo.pop()?.inverse()
    }

    /// What an undo did: its inverse becomes the next redo.
    pub fn undone(&mut self, done: &Op) {
        if let Some(redo) = done.inverse().filter(|redo| !redo.is_noop()) {
            self.redo.push(redo);
        }
    }

    /// The operation to run again. Report what it did with [`Self::redone`].
    pub fn redo(&mut self) -> Option<Op> {
        self.redo.pop()
    }

    /// What a redo did: it can be undone again (redo keeps the rest of its history).
    pub fn redone(&mut self, done: Op) {
        self.push_undo(done);
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
    fn undo_redo_and_depth() {
        let mut stack = UndoStack::new(2);
        stack.record(folder(1));
        stack.record(folder(2));
        stack.record(folder(3));
        let undo = stack.undo();
        assert!(matches!(&undo, Some(Op::Recycle { paths, .. }) if paths[0] == path(3)));
        // The undo ran and put the folder in slot $R3.
        let slot = Binned {
            original: path(3),
            slot: PathBuf::from(r"C:\$Recycle.Bin\S-1\$R3"),
        };
        stack.undone(&Op::Recycle {
            paths: vec![path(3)],
            binned: vec![slot.clone()],
        });
        // Redo brings that very item back from its slot.
        assert_eq!(
            stack.redo(),
            Some(Op::Restore {
                items: vec![slot.clone()],
                restored: vec![],
            })
        );
        stack.redone(Op::Restore {
            items: vec![slot],
            restored: vec![path(3)],
        });
        assert!(matches!(stack.undo(), Some(Op::Recycle { paths, .. }) if paths[0] == path(3)));
        assert!(stack.undo().is_some());
        assert!(stack.undo().is_none(), "depth 2 forgot the first one");
    }

    #[test]
    fn nothing_done_is_not_recorded() {
        let mut stack = UndoStack::new(4);
        stack.record(folder(1));
        let _ = stack.undo();
        stack.undone(&folder(1));
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
        let _ = stack.undo();
        stack.undone(&Op::nothing());
        stack.undone(&Op::Recycle {
            paths: vec![path(2)],
            binned: vec![],
        });
        assert!(!stack.can_undo() && !stack.can_redo());
    }
}
