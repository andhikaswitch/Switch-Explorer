//! File operations as data: what to do, recorded in the journal, and reversed for undo
//! (FEATURES.md F-OPS-07).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    pub from: PathBuf,
    pub to: PathBuf,
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
    /// Copies; `replaced` items were moved to the Recycle Bin first (the user chose Replace),
    /// at `replaced_at` (a [`crate::Trash`] stamp, set when it ran).
    Copy {
        pairs: Vec<Pair>,
        #[serde(default)]
        replaced: Vec<PathBuf>,
        #[serde(default)]
        replaced_at: Option<u64>,
    },
    /// Moves; `replaced` as for `Copy`.
    Move {
        pairs: Vec<Pair>,
        #[serde(default)]
        replaced: Vec<PathBuf>,
        #[serde(default)]
        replaced_at: Option<u64>,
    },
    /// Moves to the Recycle Bin; `at` is the trash stamp, set when it ran.
    Recycle {
        paths: Vec<PathBuf>,
        #[serde(default)]
        at: Option<u64>,
    },
    /// Brings recycled items back to these original paths (undo of `Recycle`): for each path,
    /// the earliest item recycled at or after `since` (the most recent one without it).
    Restore {
        paths: Vec<PathBuf>,
        #[serde(default)]
        since: Option<u64>,
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
    /// The operation that undoes this one, or `None` for permanent deletes.
    pub fn inverse(&self) -> Option<Op> {
        let restore_replaced = |replaced: &Vec<PathBuf>, at: &Option<u64>| {
            (!replaced.is_empty()).then(|| Op::Restore {
                paths: replaced.clone(),
                since: *at,
            })
        };
        Some(match self {
            Self::Rename { from, to } => Self::Rename {
                from: to.clone(),
                to: from.clone(),
            },
            Self::NewFolder { path } => Self::Recycle {
                paths: vec![path.clone()],
                at: None,
            },
            Self::Copy {
                pairs,
                replaced,
                replaced_at,
            } => {
                let recycle = Self::Recycle {
                    paths: pairs.iter().map(|p| p.to.clone()).collect(),
                    at: None,
                };
                match restore_replaced(replaced, replaced_at) {
                    Some(restore) => Self::Batch {
                        ops: vec![recycle, restore],
                    },
                    None => recycle,
                }
            }
            Self::Move {
                pairs,
                replaced,
                replaced_at,
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
                    replaced_at: None,
                };
                match restore_replaced(replaced, replaced_at) {
                    Some(restore) => Self::Batch {
                        ops: vec![back, restore],
                    },
                    None => back,
                }
            }
            Self::Recycle { paths, at } => Self::Restore {
                paths: paths.clone(),
                since: *at,
            },
            Self::Restore { paths, .. } => Self::Recycle {
                paths: paths.clone(),
                at: None,
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
            Self::Recycle { paths, .. } | Self::Restore { paths, .. } | Self::Delete { paths } => {
                paths.len()
            }
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

    #[test]
    fn inverses() {
        let rename = Op::Rename {
            from: p(r"C:\a"),
            to: p(r"C:\b"),
        };
        assert_eq!(rename.inverse().unwrap().inverse().unwrap(), rename);
        assert_eq!(
            Op::Recycle {
                paths: vec![p(r"C:\a")],
                at: Some(7),
            }
            .inverse(),
            Some(Op::Restore {
                paths: vec![p(r"C:\a")],
                since: Some(7),
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
            replaced_at: Some(1),
        };
        let Some(Op::Batch { ops }) = copy.inverse() else {
            panic!("expected a batch");
        };
        assert!(matches!(ops[0], Op::Recycle { .. }));
        assert!(matches!(ops[1], Op::Restore { .. }));
    }

    #[test]
    fn journal_format_is_stable() {
        let op = Op::NewFolder {
            path: p(r"C:\New folder"),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, r#"{"kind":"new_folder","path":"C:\\New folder"}"#);
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
    }
}
