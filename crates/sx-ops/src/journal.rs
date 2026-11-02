//! Write-ahead journal of file operations (ARCHITECTURE.md §8.2, §13): a `begin` line before an
//! operation runs and an `end` line after, in `journal\journal.jsonl`. After a crash, operations
//! that began but never ended are reported so the user can review them.

use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::op::Op;

/// The file is rotated (renamed to `.old`, replacing the previous one) past this size.
const ROTATE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Record {
    Begin {
        id: u64,
        at: u64,
        op: Op,
    },
    /// What was actually done (may be less than planned after a cancel or an error).
    End {
        id: u64,
        at: u64,
        done: Op,
        cancelled: bool,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
    next_id: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Journal {
    /// Opens (or creates) the journal in `dir`.
    pub fn open(dir: &Path) -> Result<Self, OpsError> {
        std::fs::create_dir_all(dir).map_err(|e| OpsError::io(dir, e))?;
        let path = dir.join("journal.jsonl");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
            let _ = std::fs::rename(&path, path.with_extension("jsonl.old"));
        }
        let next_id = read(&path)
            .iter()
            .map(|r| match r {
                Record::Begin { id, .. } | Record::End { id, .. } => *id,
            })
            .max()
            .map_or(1, |id| id + 1);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| OpsError::io(&path, e))?;
        // After a crash mid-write the last line is torn; start the next record on a new line.
        let torn = std::fs::read(&path).is_ok_and(|b| b.last().is_some_and(|&c| c != b'\n'));
        if torn {
            file.write_all(b"\n").map_err(|e| OpsError::io(&path, e))?;
        }
        Ok(Self {
            path,
            file,
            next_id,
        })
    }

    /// Records an operation before it runs; returns its id.
    pub fn begin(&mut self, op: &Op) -> Result<u64, OpsError> {
        let id = self.next_id;
        self.next_id += 1;
        self.append(&Record::Begin {
            id,
            at: now(),
            op: op.clone(),
        })?;
        Ok(id)
    }

    pub fn end(
        &mut self,
        id: u64,
        done: &Op,
        cancelled: bool,
        error: Option<String>,
    ) -> Result<(), OpsError> {
        self.append(&Record::End {
            id,
            at: now(),
            done: done.clone(),
            cancelled,
            error,
        })
    }

    fn append(&mut self, record: &Record) -> Result<(), OpsError> {
        let mut line = serde_json::to_vec(record).map_err(OpsError::Journal)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| OpsError::io(&self.path, e))
    }

    /// Operations that began but never ended (the app stopped while they ran).
    pub fn unfinished(&self) -> Vec<(u64, Op)> {
        let records = read(&self.path);
        records
            .iter()
            .filter_map(|r| match r {
                Record::Begin { id, op, .. } => Some((*id, op.clone())),
                Record::End { .. } => None,
            })
            .filter(|(id, _)| {
                !records
                    .iter()
                    .any(|r| matches!(r, Record::End { id: end, .. } if end == id))
            })
            .collect()
    }

    /// Marks an unfinished operation as reviewed so it is not reported again.
    pub fn dismiss(&mut self, id: u64, op: &Op) -> Result<(), OpsError> {
        self.end(id, op, true, Some("interrupted".into()))
    }
}

/// Every readable record; a torn last line (crash mid-write) is ignored.
fn read(path: &Path) -> Vec<Record> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_operations_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let op = Op::NewFolder {
            path: PathBuf::from(r"C:\x"),
        };
        {
            let mut journal = Journal::open(dir.path()).unwrap();
            let done = journal.begin(&op).unwrap();
            journal.end(done, &op, false, None).unwrap();
            journal.begin(&op).unwrap(); // the app "crashes" here
        }
        // A torn line from the crash.
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("journal.jsonl"))
            .unwrap()
            .write_all(b"{\"event\":\"beg")
            .unwrap();
        let mut journal = Journal::open(dir.path()).unwrap();
        let unfinished = journal.unfinished();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].0, 2);
        journal.dismiss(2, &op).unwrap();
        assert_eq!(journal.unfinished().len(), 0);
        assert_eq!(journal.begin(&op).unwrap(), 3);
    }
}
