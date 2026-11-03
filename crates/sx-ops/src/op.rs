//! File operations as data: what to do, recorded in the journal, and reversed for undo
//! (FEATURES.md F-OPS-07).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// A recycled item: where it was, and its own slot in the Recycle Bin. Undo brings back exactly
/// this item, never another one recycled from the same path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binned {
    pub original: PathBuf,
    pub slot: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Op {
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    NewFolder {
        path: PathBuf,
    },
    /// Copies; `replaced` items are moved to the Recycle Bin first (the user chose Replace).
    /// `replaced_slots` is where they went, set when it ran.
    Copy {
        pairs: Vec<Pair>,
        #[serde(default)]
        replaced: Vec<PathBuf>,
        #[serde(default)]
        replaced_slots: Vec<Binned>,
    },
    /// Moves; `replaced` as for `Copy`.
    Move {
        pairs: Vec<Pair>,
        #[serde(default)]
        replaced: Vec<PathBuf>,
        #[serde(default)]
        replaced_slots: Vec<Binned>,
    },
    /// Moves to the Recycle Bin; `binned` is where each item went, set when it ran.
    Recycle {
        paths: Vec<PathBuf>,
        #[serde(default)]
        binned: Vec<Binned>,
    },
    /// Brings recycled items back (undo of `Recycle`). `restored` is where each one came back,
    /// set when it ran: its original path, or "name (2)" when that name was taken again.
    Restore {
        #[serde(default)]
        items: Vec<Binned>,
        #[serde(default)]
        restored: Vec<PathBuf>,
    },
    /// Permanent delete; cannot be undone.
    Delete {
        paths: Vec<PathBuf>,
    },
    /// Several operations run in order (an undo that recycles copies and restores what they
    /// replaced).
    Batch {
        ops: Vec<Op>,
    },
}

impl Op {
    /// An operation that did nothing (what an operation that failed at once has done).
    pub fn nothing() -> Self {
        Self::Batch { ops: Vec::new() }
    }

    /// Whether running this changed nothing, so there is nothing to undo.
    pub fn is_noop(&self) -> bool {
        match self {
            Self::Rename { .. } | Self::NewFolder { .. } => false,
            Self::Copy {
                pairs,
                replaced_slots,
                ..
            }
            | Self::Move {
                pairs,
                replaced_slots,
                ..
            } => pairs.is_empty() && replaced_slots.is_empty(),
            Self::Recycle { paths, .. } | Self::Delete { paths } => paths.is_empty(),
            Self::Restore { items, .. } => items.is_empty(),
            Self::Batch { ops } => ops.iter().all(Op::is_noop),
        }
    }

    /// The operation that undoes this one, or `None` for permanent deletes. Built from what an
    /// operation did (the `Outcome` of running it), so recycled items come back from their own
    /// slots and restored items are recycled again under the name they came back with.
    pub fn inverse(&self) -> Option<Op> {
        let restore_replaced = |slots: &Vec<Binned>| {
            (!slots.is_empty()).then(|| Op::Restore {
                items: slots.clone(),
                restored: Vec::new(),
            })
        };
        Some(match self {
            Self::Rename { from, to } => Self::Rename {
                from: to.clone(),
                to: from.clone(),
            },
            Self::NewFolder { path } => Self::Recycle {
                paths: vec![path.clone()],
                binned: Vec::new(),
            },
            Self::Copy {
                pairs,
                replaced_slots,
                ..
            } => {
                let recycle = Self::Recycle {
                    paths: pairs.iter().map(|p| p.to.clone()).collect(),
                    binned: Vec::new(),
                };
                match restore_replaced(replaced_slots) {
                    Some(restore) => Self::Batch {
                        ops: vec![recycle, restore],
                    },
                    None => recycle,
                }
            }
            Self::Move {
                pairs,
                replaced_slots,
                ..
            } => {
                let back = Self::Move {
                    pairs: pairs
                        .iter()
                        .map(|p| Pair {
                            from: p.to.clone(),
                            to: p.from.clone(),
                        })
                        .collect(),
                    replaced: Vec::new(),
                    replaced_slots: Vec::new(),
                };
                match restore_replaced(replaced_slots) {
                    Some(restore) => Self::Batch {
                        ops: vec![back, restore],
                    },
                    None => back,
                }
            }
            Self::Recycle { binned, .. } => Self::Restore {
                items: binned.clone(),
                restored: Vec::new(),
            },
            Self::Restore { restored, .. } => Self::Recycle {
                paths: restored.clone(),
                binned: Vec::new(),
            },
            Self::Delete { .. } => return None,
            Self::Batch { ops } => Self::Batch {
                ops: ops.iter().rev().map(Op::inverse).collect::<Option<_>>()?,
            },
        })
    }

    /// Top-level items the operation touches (for progress and messages).
    pub fn item_count(&self) -> usize {
        match self {
            Self::Rename { .. } | Self::NewFolder { .. } => 1,
            Self::Copy { pairs, .. } | Self::Move { pairs, .. } => pairs.len(),
            Self::Recycle { paths, .. } | Self::Delete { paths } => paths.len(),
            Self::Restore { items, .. } => items.len(),
            Self::Batch { ops } => ops.iter().map(Op::item_count).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn binned(original: &str, slot: &str) -> Binned {
        Binned {
            original: p(original),
            slot: p(slot),
        }
    }

    #[test]
    fn inverses() {
        let rename = Op::Rename {
            from: p(r"C:\a"),
            to: p(r"C:\b"),
        };
        assert_eq!(rename.inverse().unwrap().inverse().unwrap(), rename);
        let slot = binned(r"C:\a", r"C:\$Recycle.Bin\S-1\$R1");
        assert_eq!(
            Op::Recycle {
                paths: vec![p(r"C:\a")],
                binned: vec![slot.clone()],
            }
            .inverse(),
            Some(Op::Restore {
                items: vec![slot.clone()],
                restored: vec![],
            })
        );
        // A restore that had to rename recycles the renamed item, not whatever now has the name.
        assert_eq!(
            Op::Restore {
                items: vec![slot],
                restored: vec![p(r"C:\a (2)")],
            }
            .inverse(),
            Some(Op::Recycle {
                paths: vec![p(r"C:\a (2)")],
                binned: vec![],
            })
        );
        assert_eq!(Op::Delete { paths: vec![] }.inverse(), None);
    }

    #[test]
    fn replacing_copies_undo_in_two_steps() {
        let copy = Op::Copy {
            pairs: vec![Pair {
                from: p(r"C:\src\a.txt"),
                to: p(r"D:\dst\a.txt"),
            }],
            replaced: vec![p(r"D:\dst\a.txt")],
            replaced_slots: vec![binned(r"D:\dst\a.txt", r"D:\$Recycle.Bin\S-1\$R1.txt")],
        };
        let Some(Op::Batch { ops }) = copy.inverse() else {
            panic!("expected a batch");
        };
        assert!(matches!(ops[0], Op::Recycle { .. }));
        assert!(matches!(&ops[1], Op::Restore { items, .. } if items.len() == 1));
    }

    #[test]
    fn noops() {
        assert!(Op::nothing().is_noop());
        assert!(
            Op::Recycle {
                paths: vec![],
                binned: vec![],
            }
            .is_noop()
        );
        // A copy that stopped after recycling what it was replacing still has something to undo.
        assert!(
            !Op::Copy {
                pairs: vec![],
                replaced: vec![p(r"D:\a")],
                replaced_slots: vec![binned(r"D:\a", r"D:\$R1")],
            }
            .is_noop()
        );
    }

    #[test]
    fn journal_format_is_stable() {
        let op = Op::NewFolder {
            path: p(r"C:\New folder"),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, r#"{"kind":"new_folder","path":"C:\\New folder"}"#);
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
        // Records written before items carried their slot still read (without undo data).
        let old = r#"{"kind":"recycle","paths":["C:\\a"],"at":1762000000}"#;
        assert_eq!(
            serde_json::from_str::<Op>(old).unwrap(),
            Op::Recycle {
                paths: vec![p(r"C:\a")],
                binned: vec![],
            }
        );
    }
}
