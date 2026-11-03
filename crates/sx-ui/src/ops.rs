//! File operations from the UI (FEATURES.md F-OPS): commands, the running jobs, undo/redo,
//! the journal and crash recovery. Work runs on its own STA thread per job (the Recycle Bin
//! needs COM); progress and the result come back over a channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext as _, ClipboardItem, Context, Entity, SharedString, Subscription, Window,
};
use rust_i18n::t;
use sx_core::Location;
use sx_ops::name::NameError;
use sx_ops::{
    Binned, Cancel, Conflict, Journal, Mode, Op, OpsError, Outcome, Progress, Resolution, UndoStack,
};
use sx_shell::ShellError;
use sx_shell::clipboard::{self, ClipboardFiles};

use crate::workspace::Workspace;

/// How long the "Done · Undo" toast stays.
const TOAST_FOR: Duration = Duration::from_secs(8);

/// Why a job runs: user work is recorded for undo; undo and redo runs are not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    User,
    Undo,
    Redo,
}

#[derive(Debug)]
pub(crate) struct Job {
    pub id: u64,
    pub label: SharedString,
    pub progress: Progress,
    pub cancel: Arc<Cancel>,
    pub finished: Option<SharedString>,
}

/// A paste waiting for conflict decisions (FEATURES.md F-OPS-05).
#[derive(Debug)]
pub(crate) struct PendingPaste {
    pub sources: Vec<PathBuf>,
    pub dest: PathBuf,
    pub mode: Mode,
    pub conflicts: Vec<Conflict>,
    pub index: usize,
    pub decisions: HashMap<PathBuf, Resolution>,
    pub apply_all: bool,
    /// For a cut: the clipboard sequence when it was read, to clear it after the move.
    pub clipboard_sequence: Option<u32>,
}

#[derive(Debug)]
pub(crate) enum Dialog {
    ConfirmRecycle(Vec<PathBuf>),
    ConfirmDelete(Vec<PathBuf>),
    Conflict(PendingPaste),
}

/// The message after an operation, with Undo when it can be undone.
#[derive(Debug)]
pub(crate) struct Toast {
    pub id: u64,
    pub text: SharedString,
    pub undo: bool,
}

#[derive(Debug)]
pub(crate) struct Renaming {
    pub tab_id: u64,
    /// Entry index in the snapshot the rename started on.
    pub index: u32,
    pub from: PathBuf,
    pub original: String,
    pub input: Entity<InputState>,
    _subscription: Subscription,
}

#[derive(Debug, Default)]
pub(crate) struct OpsState {
    journal: Option<Journal>,
    pub undo: UndoStack,
    pub jobs: Vec<Job>,
    next_job: u64,
    pub dialog: Option<Dialog>,
    pub toast: Option<Toast>,
    pub transfer_panel: bool,
    pub renaming: Option<Renaming>,
    /// Operations that began before a crash and never ended (from the journal).
    pub interrupted: Vec<(u64, Op)>,
    /// Cut-paste jobs and the clipboard sequence to clear once they have moved everything.
    clear_clipboard_after: HashMap<u64, u32>,
}

impl OpsState {
    pub fn new(journal_dir: Option<&Path>, undo_depth: usize) -> Self {
        let journal = journal_dir.and_then(|dir| {
            Journal::open(dir)
                .map_err(|err| tracing::warn!(error = %err, "journal unavailable"))
                .ok()
        });
        let interrupted = journal
            .as_ref()
            .map(Journal::unfinished)
            .unwrap_or_default();
        Self {
            journal,
            undo: UndoStack::new(undo_depth),
            interrupted,
            ..Self::default()
        }
    }

    pub fn running(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter().filter(|j| j.finished.is_none())
    }
}

/// The Recycle Bin of the shell, used from the job's STA thread.
struct ShellTrash {
    owner: Option<isize>,
}

impl sx_ops::Trash for ShellTrash {
    fn recycle(&self, paths: &[PathBuf]) -> sx_ops::Recycled {
        let recycled = sx_shell::recycle::recycle(paths, self.owner);
        sx_ops::Recycled {
            binned: recycled
                .binned
                .into_iter()
                .map(|(original, slot)| Binned { original, slot })
                .collect(),
            deleted: recycled.deleted,
            cancelled: recycled.cancelled,
            error: recycled.error.map(trash_error),
        }
    }

    fn restore(&self, items: &[Binned]) -> sx_ops::Restored {
        let request: Vec<(PathBuf, PathBuf)> = items
            .iter()
            .map(|b| (b.original.clone(), b.slot.clone()))
            .collect();
        let restored = sx_shell::recycle::restore(&request);
        sx_ops::Restored {
            done: restored
                .done
                .into_iter()
                .filter_map(|(ix, back)| Some((items.get(ix)?.clone(), back)))
                .collect(),
            error: restored.error.map(trash_error),
        }
    }
}

/// Shell failures as the engine's errors, keeping the item and the Win32 code for the message.
fn trash_error(err: ShellError) -> OpsError {
    let code = err.win32_code();
    match err {
        ShellError::NotRecycled(path) => OpsError::NotInRecycleBin(path),
        ShellError::Item { path, .. } | ShellError::Open { path, .. } if code.is_some() => {
            let code = code.map_or(0, |c| c as i32);
            OpsError::io(&path, std::io::Error::from_raw_os_error(code))
        }
        other => OpsError::Trash(other.to_string()),
    }
}

enum JobEvent {
    Progress(Progress),
    Finished(Outcome),
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The label of a running job, e.g. "Copying 3 items".
fn job_label(op: &Op, origin: Origin) -> SharedString {
    let count = op.item_count();
    let key = match (origin, op) {
        (Origin::Undo, _) => "ops.undoing",
        (_, Op::Copy { .. }) => "ops.copying",
        (_, Op::Move { .. }) => "ops.moving",
        (_, Op::Recycle { .. }) => "ops.recycling",
        (_, Op::Restore { .. }) => "ops.restoring",
        (_, Op::Delete { .. }) => "ops.deleting",
        (_, Op::Rename { .. } | Op::NewFolder { .. } | Op::Batch { .. }) => "ops.working",
    };
    if count == 1 {
        t!(format!("{key}_one"), count = count).into()
    } else {
        t!(key, count = count).into()
    }
}

/// Runs blocking work on its own thread and resolves with its result (`None` if it panicked or
/// the thread could not start). For the clipboard, which can wait on another app, and for file
/// system checks, which can take seconds on a network folder: never on the UI thread.
fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = Option<T>> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("sx-blocking".into())
        .spawn(move || {
            if let Ok(value) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                let _ = tx.send(value);
            }
        });
    if let Err(err) = &spawned {
        tracing::error!(error = %err, "cannot start a worker thread");
    }
    async move {
        spawned.ok()?;
        rx.await.ok()
    }
}

/// What `paste` read: the files, the conflicts with the destination, the clipboard sequence.
type ClipboardRead = Result<Option<(ClipboardFiles, Vec<Conflict>, u32)>, sx_shell::ShellError>;

/// The message once a job is done.
fn done_text(op: &Op, origin: Origin) -> SharedString {
    let count = op.item_count();
    let plural = |key: &str| {
        if count == 1 {
            t!(format!("{key}_one"), count = count)
        } else {
            t!(key, count = count)
        }
    };
    match (origin, op) {
        (Origin::Undo, _) => t!("ops.undone"),
        (_, Op::Copy { .. }) => plural("ops.done_copy"),
        (_, Op::Move { .. }) => plural("ops.done_move"),
        (_, Op::Recycle { .. }) => plural("ops.done_recycle"),
        (_, Op::Restore { .. }) => plural("ops.done_restore"),
        (_, Op::Delete { .. }) => plural("ops.done_delete"),
        (_, Op::Rename { to, .. }) => t!("ops.done_rename", name = file_name(to)),
        (_, Op::NewFolder { .. } | Op::Batch { .. }) => t!("ops.done"),
    }
    .into()
}

/// The note added when the Recycle Bin could not take some items and the user agreed to delete
/// them permanently in the shell's warning.
fn deleted_text(count: usize) -> String {
    if count == 1 {
        t!("ops.deleted_instead_one").into_owned()
    } else {
        t!("ops.deleted_instead", count = count).into_owned()
    }
}

pub(crate) fn name_error(err: &NameError) -> SharedString {
    match err {
        NameError::Empty => t!("ops.name_empty"),
        NameError::InvalidChar(c) => t!("ops.name_invalid_char", char = c.to_string()),
        NameError::Reserved(name) => t!("ops.name_reserved", name = name),
        NameError::TrailingDotOrSpace => t!("ops.name_trailing"),
        NameError::TooLong => t!("ops.name_too_long"),
    }
    .into()
}

fn ops_error(err: &OpsError) -> SharedString {
    let name = err.path().map(file_name).unwrap_or_default();
    match err {
        OpsError::Exists(_) => t!("ops.exists", name = name),
        OpsError::IntoItself(_) => t!("ops.into_itself", name = name),
        OpsError::VerifyFailed(_) => t!("ops.verify_failed", name = name),
        OpsError::NotInRecycleBin(_) => t!("ops.not_in_bin", name = name),
        OpsError::ReadOnly => t!("ops.readonly"),
        OpsError::Io { source, .. } => match source.kind() {
            std::io::ErrorKind::PermissionDenied => t!("ops.access_denied", name = name),
            std::io::ErrorKind::NotFound => t!("ops.missing", name = name),
            _ => t!(
                "ops.failed",
                name = name,
                code = source.raw_os_error().unwrap_or(0)
            ),
        },
        OpsError::Trash(_) | OpsError::Journal(_) => t!("ops.failed", name = name, code = 0),
    }
    .into()
}

impl Workspace {
    /// Paths of the selected rows in the active tab, built from exact names.
    pub(crate) fn selected_paths(&self) -> Vec<PathBuf> {
        let tab = self.tab();
        let (Some(snapshot), Some(dir)) = (tab.snapshot(), tab.location().as_path()) else {
            return Vec::new();
        };
        tab.selection
            .iter()
            .filter_map(|row| snapshot.row(row as usize))
            .filter_map(|entry| snapshot.path_of(dir, entry.index))
            .collect()
    }

    fn writable(&mut self, cx: &mut Context<Self>) -> bool {
        if self.features.safety_readonly_mode {
            self.notice = Some(t!("ops.readonly").into());
            cx.notify();
            return false;
        }
        true
    }

    /// Starts an operation on its own STA thread; returns the job id.
    pub(crate) fn run_op(&mut self, op: Op, origin: Origin, cx: &mut Context<Self>) -> Option<u64> {
        if !self.writable(cx) {
            return None;
        }
        let journal_id = self.ops.journal.as_mut().and_then(|j| {
            j.begin(&op)
                .map_err(|e| tracing::warn!(error = %e, "journal"))
                .ok()
        });
        let id = self.ops.next_job;
        self.ops.next_job += 1;
        let cancel = Arc::new(Cancel::default());
        self.ops.jobs.push(Job {
            id,
            label: job_label(&op, origin),
            progress: Progress {
                total: op.item_count(),
                ..Progress::default()
            },
            cancel: cancel.clone(),
            finished: None,
        });
        // Finished jobs older than the last few are forgotten.
        let finished = self
            .ops
            .jobs
            .iter()
            .filter(|j| j.finished.is_some())
            .count();
        if finished > 20
            && let Some(ix) = self.ops.jobs.iter().position(|j| j.finished.is_some())
        {
            self.ops.jobs.remove(ix);
        }
        let (tx, mut rx) = mpsc::unbounded::<JobEvent>();
        let owner = self.hwnd;
        let planned = op;
        let spawned = std::thread::Builder::new()
            .name("sx-op".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _com = sx_shell::StaGuard::new();
                    let trash = ShellTrash { owner };
                    let progress_tx = tx.clone();
                    sx_ops::run(&planned, &trash, &cancel, &mut |p: &Progress| {
                        let _ = progress_tx.unbounded_send(JobEvent::Progress(p.clone()));
                    })
                }))
                .unwrap_or_else(|_| Outcome {
                    done: Op::nothing(),
                    cancelled: false,
                    error: Some(OpsError::Trash("the operation stopped unexpectedly".into())),
                    deleted: Vec::new(),
                });
                let _ = tx.unbounded_send(JobEvent::Finished(result));
            });
        if let Err(err) = spawned {
            tracing::error!(error = %err, "cannot start an operation thread");
            return None;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                let alive = this.update(cx, |this, cx| match event {
                    JobEvent::Progress(progress) => {
                        if let Some(job) = this.ops.jobs.iter_mut().find(|j| j.id == id) {
                            job.progress = progress;
                        }
                        cx.notify();
                    }
                    JobEvent::Finished(outcome) => {
                        this.on_job_finished(id, journal_id, origin, &outcome, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Some(id)
    }

    /// Records what the job did (journal, undo or redo) and tells the user. A job that stopped
    /// early still records the part it did, so that part can be undone.
    fn on_job_finished(
        &mut self,
        id: u64,
        journal_id: Option<u64>,
        origin: Origin,
        outcome: &Outcome,
        cx: &mut Context<Self>,
    ) {
        let done = &outcome.done;
        match origin {
            Origin::User if self.features.ops_undo => self.ops.undo.record(done.clone()),
            Origin::User => {}
            Origin::Undo => self.ops.undo.undone(done),
            Origin::Redo => self.ops.undo.redone(done.clone()),
        }
        let mut message: String = match &outcome.error {
            Some(err) => {
                tracing::debug!(error = %err, "operation failed");
                ops_error(err).into()
            }
            None if outcome.cancelled => t!("ops.cancelled").into_owned(),
            None => done_text(done, origin).into(),
        };
        if !outcome.deleted.is_empty() {
            message.push(' ');
            message.push_str(&deleted_text(outcome.deleted.len()));
        }
        let message = SharedString::from(message);
        let undo = origin != Origin::Undo
            && self.features.ops_undo
            && !done.is_noop()
            && done.inverse().is_some();
        if let (Some(journal), Some(journal_id)) = (self.ops.journal.as_mut(), journal_id) {
            let error = outcome.error.as_ref().map(ToString::to_string);
            if let Err(err) = journal.end(journal_id, done, outcome.cancelled, error) {
                tracing::warn!(error = %err, "journal");
            }
        }
        if let Some(job) = self.ops.jobs.iter_mut().find(|j| j.id == id) {
            job.finished = Some(message.clone());
        }
        // A cut is used up once everything moved; an error or a cancel keeps it for another try.
        if let Some(sequence) = self.ops.clear_clipboard_after.remove(&id)
            && !outcome.stopped_early()
        {
            let owner = self.hwnd;
            cx.spawn(async move |_, _| {
                if let Some(Err(err)) =
                    off_thread(move || clipboard::clear_if_unchanged(sequence, owner)).await
                {
                    tracing::debug!(error = %err, "clipboard");
                }
            })
            .detach();
        }
        if outcome.error.is_some() && done.is_noop() {
            self.notice = Some(message);
        } else {
            // Also after an error when part of the job was done: that part can be undone.
            self.show_toast(message, undo, cx);
        }
        // Without a watcher (perf.watcher off) the folder would not update by itself.
        if !self.features.perf_watcher {
            let tab_id = self.tab().id;
            self.load_tab(tab_id, true, cx);
        }
        cx.notify();
    }

    fn show_toast(&mut self, text: SharedString, undo: bool, cx: &mut Context<Self>) {
        let id = self.ops.next_job;
        self.ops.next_job += 1;
        self.ops.toast = Some(Toast { id, text, undo });
        let timer = cx.background_executor().timer(TOAST_FOR);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.ops.toast.as_ref().is_some_and(|t| t.id == id) {
                    this.ops.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ---- Commands -----------------------------------------------------------------------

    pub(crate) fn delete_selection(&mut self, permanent: bool, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() || !self.writable(cx) {
            return;
        }
        let recycle = !permanent && self.features.ops_delete_to_recycle_bin;
        if recycle {
            if self.features.ops_confirm_delete {
                self.ops.dialog = Some(Dialog::ConfirmRecycle(paths));
                cx.notify();
            } else {
                self.run_op(
                    Op::Recycle {
                        paths,
                        binned: Vec::new(),
                    },
                    Origin::User,
                    cx,
                );
            }
        } else if self.features.ops_confirm_permanent_delete {
            self.ops.dialog = Some(Dialog::ConfirmDelete(paths));
            cx.notify();
        } else {
            self.run_op(Op::Delete { paths }, Origin::User, cx);
        }
    }

    pub(crate) fn copy_selection(&mut self, cut: bool, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let owner = self.hwnd;
        cx.spawn(async move |this, cx| {
            let written =
                off_thread(move || clipboard::write(&ClipboardFiles { paths, cut }, owner)).await;
            if let Some(Ok(())) = written {
                return;
            }
            if let Some(Err(err)) = written {
                tracing::debug!(error = %err, "clipboard");
            }
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(t!("ops.clipboard_busy").into());
                cx.notify();
            });
        })
        .detach();
    }

    /// `Ctrl+Shift+C`: the selected paths in quotes, one per line, as Windows 11 copies them.
    pub(crate) fn copy_paths(&mut self, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        let text = if paths.is_empty() {
            match self.tab().location().as_path() {
                Some(dir) => format!("\"{}\"", dir.display()),
                None => return,
            }
        } else {
            paths
                .iter()
                .map(|p| format!("\"{}\"", p.display()))
                .collect::<Vec<_>>()
                .join("\r\n")
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Reads the clipboard and checks the destination off the UI thread, then pastes or asks
    /// about conflicts.
    pub(crate) fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(dest) = self.tab().location().as_path().map(Path::to_path_buf) else {
            return;
        };
        if !self.writable(cx) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let target = dest.clone();
            let read = off_thread(move || -> ClipboardRead {
                let sequence = clipboard::sequence();
                Ok(clipboard::read()?
                    .filter(|files| !files.paths.is_empty())
                    .map(|files| {
                        let conflicts = sx_ops::plan::conflicts(&files.paths, &target);
                        (files, conflicts, sequence)
                    }))
            })
            .await;
            let _ = this.update(cx, |this, cx| this.on_clipboard_read(dest, read, cx));
        })
        .detach();
    }

    fn on_clipboard_read(
        &mut self,
        dest: PathBuf,
        read: Option<ClipboardRead>,
        cx: &mut Context<Self>,
    ) {
        let (files, conflicts, sequence) = match read {
            Some(Ok(Some(read))) => read,
            Some(Ok(None)) => return,
            Some(Err(err)) => {
                tracing::debug!(error = %err, "clipboard");
                self.notice = Some(t!("ops.clipboard_busy").into());
                cx.notify();
                return;
            }
            None => {
                self.notice = Some(t!("ops.clipboard_busy").into());
                cx.notify();
                return;
            }
        };
        let mode = if files.cut { Mode::Move } else { Mode::Copy };
        let pending = PendingPaste {
            sources: files.paths,
            dest,
            mode,
            conflicts,
            index: 0,
            decisions: HashMap::new(),
            apply_all: false,
            clipboard_sequence: files.cut.then_some(sequence),
        };
        if pending.conflicts.is_empty() {
            self.finish_paste(pending, cx);
        } else {
            self.ops.dialog = Some(Dialog::Conflict(pending));
            cx.notify();
        }
    }

    /// Records the decision for the current conflict and moves on (or runs the paste).
    pub(crate) fn decide_conflict(&mut self, resolution: Resolution, cx: &mut Context<Self>) {
        let Some(Dialog::Conflict(mut pending)) = self.ops.dialog.take() else {
            return;
        };
        let rest: Vec<PathBuf> = if pending.apply_all {
            pending.conflicts[pending.index..]
                .iter()
                .map(|c| c.source.clone())
                .collect()
        } else {
            vec![pending.conflicts[pending.index].source.clone()]
        };
        for source in rest {
            pending.decisions.insert(source, resolution);
        }
        pending.index = pending
            .conflicts
            .iter()
            .position(|c| !pending.decisions.contains_key(&c.source))
            .unwrap_or(pending.conflicts.len());
        if pending.index < pending.conflicts.len() {
            self.ops.dialog = Some(Dialog::Conflict(pending));
        } else {
            self.finish_paste(pending, cx);
        }
        cx.notify();
    }

    /// Plans the paste with the decisions (off the UI thread: "Keep both" checks names on disk)
    /// and runs it.
    fn finish_paste(&mut self, pending: PendingPaste, cx: &mut Context<Self>) {
        let sequence = pending.clipboard_sequence;
        cx.spawn(async move |this, cx| {
            let planned = off_thread(move || {
                let decisions = pending.decisions;
                sx_ops::plan::paste(&pending.sources, &pending.dest, pending.mode, |c| {
                    decisions
                        .get(&c.source)
                        .copied()
                        .unwrap_or(Resolution::Skip)
                })
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let Some(Some(op)) = planned else {
                    return;
                };
                if let (Some(id), Some(sequence)) = (this.run_op(op, Origin::User, cx), sequence) {
                    this.ops.clear_clipboard_after.insert(id, sequence);
                }
            });
        })
        .detach();
    }

    pub(crate) fn confirm_dialog(&mut self, cx: &mut Context<Self>) {
        match self.ops.dialog.take() {
            Some(Dialog::ConfirmRecycle(paths)) => {
                self.run_op(
                    Op::Recycle {
                        paths,
                        binned: Vec::new(),
                    },
                    Origin::User,
                    cx,
                );
            }
            Some(Dialog::ConfirmDelete(paths)) => {
                self.run_op(Op::Delete { paths }, Origin::User, cx);
            }
            Some(conflict @ Dialog::Conflict(_)) => self.ops.dialog = Some(conflict),
            None => {}
        }
        cx.notify();
    }

    pub(crate) fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.ops.dialog = None;
        cx.notify();
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        self.ops.toast = None;
        if let Some(inverse) = self.ops.undo.undo() {
            self.run_op(inverse, Origin::Undo, cx);
        } else {
            self.notice = Some(t!("ops.nothing_to_undo").into());
            cx.notify();
        }
    }

    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(op) = self.ops.undo.redo() {
            self.run_op(op, Origin::Redo, cx);
        }
    }

    /// `Ctrl+Shift+N`: a "New folder" (numbered if taken) that opens in rename mode.
    pub(crate) fn new_folder(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.tab().location().as_path().map(Path::to_path_buf) else {
            return;
        };
        if !self.writable(cx) {
            return;
        }
        let tab_id = self.tab().id;
        let base = t!("ops.new_folder_name").into_owned();
        cx.spawn(async move |this, cx| {
            // Finding a free number checks names on disk: off the UI thread.
            let folder = dir.clone();
            let Some(name) =
                off_thread(move || sx_ops::name::keep_both(&folder, &base, Path::exists)).await
            else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.rename_after_load = Some(name.clone());
                }
                this.run_op(
                    Op::NewFolder {
                        path: dir.join(name),
                    },
                    Origin::User,
                    cx,
                );
            });
        })
        .detach();
    }

    // ---- Inline rename (F2) ---------------------------------------------------------------

    pub(crate) fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let (Some(snapshot), Some(dir)) = (tab.snapshot(), tab.location().as_path()) else {
            return;
        };
        let Some(row) = tab
            .selection
            .focus()
            .or_else(|| tab.selection.iter().next())
        else {
            return;
        };
        let Some(entry) = snapshot.row(row as usize) else {
            return;
        };
        let Some(from) = snapshot.path_of(dir, entry.index) else {
            return;
        };
        let (index, original, tab_id) = (entry.index, entry.name.to_string(), tab.id);
        let stem = if entry.kind.is_container() {
            original.len()
        } else {
            sx_ops::name::stem_len(&original)
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |state, cx| {
            state.set_value(original.clone(), window, cx);
            state.set_selected_range(0..stem, cx);
            state.focus(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_rename(window, cx),
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        self.ops.renaming = Some(Renaming {
            tab_id,
            index,
            from,
            original,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.renaming.take().is_some() {
            window.focus(&self.list_focus, cx);
            cx.notify();
        }
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.ops.renaming.take() else {
            return;
        };
        window.focus(&self.list_focus, cx);
        let name = renaming.input.read(cx).value().trim().to_string();
        cx.notify();
        if name == renaming.original {
            return;
        }
        if let Err(err) = sx_ops::name::validate(&name) {
            self.notice = Some(name_error(&err));
            return;
        }
        let Some(dir) = renaming.from.parent() else {
            return;
        };
        // A taken name is reported by the operation itself, which checks on its worker thread.
        let to = dir.join(&name);
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == renaming.tab_id) {
            tab.reveal_after_refresh = Some(name);
        }
        self.run_op(
            Op::Rename {
                from: renaming.from,
                to,
            },
            Origin::User,
            cx,
        );
    }

    // ---- Crash recovery -------------------------------------------------------------------

    /// Shown once at start when the journal has operations that never finished.
    pub(crate) fn report_interrupted(&mut self) {
        let count = self.ops.interrupted.len();
        if count > 0 {
            self.notice = Some(t!("ops.interrupted", count = count).into());
        }
    }

    pub(crate) fn dismiss_interrupted(&mut self, cx: &mut Context<Self>) {
        let interrupted = std::mem::take(&mut self.ops.interrupted);
        if let Some(journal) = self.ops.journal.as_mut() {
            for (id, op) in &interrupted {
                let _ = journal.dismiss(*id, op);
            }
        }
        cx.notify();
    }

    pub(crate) fn open_location_of(&mut self, op: &Op, cx: &mut Context<Self>) {
        let path = match op {
            Op::Rename { to, .. } => to.parent().map(Path::to_path_buf),
            Op::NewFolder { path } => path.parent().map(Path::to_path_buf),
            Op::Copy { pairs, .. } | Op::Move { pairs, .. } => pairs
                .first()
                .and_then(|p| p.to.parent().map(Path::to_path_buf)),
            Op::Recycle { paths, .. } | Op::Delete { paths } => paths
                .first()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            Op::Restore { items, .. } => items
                .first()
                .and_then(|b| b.original.parent().map(Path::to_path_buf)),
            Op::Batch { .. } => None,
        };
        if let Some(path) = path {
            self.open_tab(Location::Path(path), cx);
        }
    }
}
