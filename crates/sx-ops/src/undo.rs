//! Undo/redo stack of completed operations (FEATURES.md F-OPS-07, `ops.undo_depth`).

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

    /// Records a completed operation; clears redo. Operations without an inverse (permanent
    /// delete) are not recorded.
    pub fn record(&mut self, done: Op) {
        if done.inverse().is_none() || done.item_count() == 0 {
            return;
        }
        if self.undo.len() == self.depth {
            self.undo.remove(0);
        }
        self.undo.push(done);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The operation that undoes the last one (it moves to the redo side).
    pub fn undo(&mut self) -> Option<Op> {
        let op = self.undo.pop()?;
        let inverse = op.inverse();
        self.redo.push(op);
        inverse
    }

    /// The operation to run again (it moves back to the undo side).
    pub fn redo(&mut self) -> Option<Op> {
        let op = self.redo.pop()?;
        self.undo.push(op.clone());
        Some(op)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn folder(n: u32) -> Op {
        Op::NewFolder {
            path: PathBuf::from(format!(r"C:\{n}")),
        }
    }

    #[test]
    fn undo_redo_and_depth() {
        let mut stack = UndoStack::new(2);
        stack.record(folder(1));
        stack.record(folder(2));
        stack.record(folder(3));
        assert!(matches!(stack.undo(), Some(Op::Recycle { paths, .. }) if paths[0].ends_with("3")));
        assert_eq!(stack.redo(), Some(folder(3)));
        stack.undo();
        stack.undo();
        assert!(stack.undo().is_none(), "depth 2 forgot the first one");
        stack.record(Op::Delete { paths: vec![] });
        assert!(stack.can_redo(), "permanent deletes are not recorded");
    }
}
