//! Session restore (FEATURES.md F-WIN-05, `tabs.restore_session`): the open tabs, the active one
//! and the window's place, in `%LOCALAPPDATA%\SwitchExplorer\session.json`. Written atomically
//! off the UI thread a moment after a change, and once more when the window closes. Only the
//! active tab is listed at start; the others are listed when they are first shown.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::{Bounds, Context, Pixels, Window, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};
use sx_core::Location;

use crate::workspace::Workspace;

/// Changes that come close together (typing a path, Back several times) are written once.
const WRITE_AFTER: Duration = Duration::from_millis(500);

/// What a restart opens again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
    #[serde(default)]
    pub window: Option<SavedWindow>,
}

/// A tab's place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum SavedTab {
    Home,
    ThisPc,
    RecycleBin,
    Folder(PathBuf),
}

impl SavedTab {
    fn from_location(location: &Location) -> Self {
        match location {
            Location::Home => Self::Home,
            Location::ThisPc => Self::ThisPc,
            Location::RecycleBin => Self::RecycleBin,
            Location::Path(path) => Self::Folder(path.clone()),
        }
    }

    pub fn location(&self) -> Location {
        match self {
            Self::Home => Location::Home,
            Self::ThisPc => Location::ThisPc,
            Self::RecycleBin => Location::RecycleBin,
            Self::Folder(path) => Location::Path(path.clone()),
        }
    }
}

/// The window's normal (not maximized) place, in logical pixels, and whether it was maximized.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl SavedWindow {
    fn of(bounds: WindowBounds) -> Self {
        let (rect, maximized) = match bounds {
            WindowBounds::Windowed(rect) => (rect, false),
            WindowBounds::Maximized(rect) | WindowBounds::Fullscreen(rect) => (rect, true),
        };
        Self {
            x: f32::from(rect.origin.x),
            y: f32::from(rect.origin.y),
            width: f32::from(rect.size.width),
            height: f32::from(rect.size.height),
            maximized,
        }
    }

    /// The window to open with, when it would still show on one of `displays` (a monitor may
    /// have been unplugged since).
    pub fn bounds(&self, displays: &[Bounds<Pixels>], min: (f32, f32)) -> Option<WindowBounds> {
        let rect = Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width.max(min.0)), px(self.height.max(min.1))),
        };
        let shows = displays.iter().any(|display| display.intersects(&rect));
        if !shows || !self.width.is_finite() || !self.height.is_finite() {
            return None;
        }
        Some(if self.maximized {
            WindowBounds::Maximized(rect)
        } else {
            WindowBounds::Windowed(rect)
        })
    }
}

/// The saved session; `None` when there is none or it cannot be read (a broken file is ignored:
/// a restart then opens Home, as on a first run).
pub fn load(file: &Path) -> Option<Session> {
    let bytes = std::fs::read(file).ok()?;
    let session: Session = serde_json::from_slice(&bytes)
        .map_err(|err| tracing::warn!(error = %err, "session.json not read"))
        .ok()?;
    (!session.tabs.is_empty()).then_some(session)
}

/// Writes the session atomically (a temporary file of its own, then rename): two writes never
/// share a temporary file.
fn save(file: &Path, session: &Session) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(session).map_err(std::io::Error::other)?;
    let tmp = file.with_extension(format!("json.{}.tmp", NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

impl Workspace {
    /// The session as it is now.
    fn session(&self) -> Session {
        Session {
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab::from_location(tab.location()))
                .collect(),
            active: self.active,
            window: self.window_place,
        }
    }

    /// Follows the window's place, for the session.
    pub(crate) fn watch_window_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_place = Some(SavedWindow::of(window.window_bounds()));
        self.window_watch = Some(cx.observe_window_bounds(window, |this, window, cx| {
            this.window_place = Some(SavedWindow::of(window.window_bounds()));
            this.session_changed(cx);
        }));
    }

    /// The first tabs: the places asked for; else the last session (`tabs.restore_session`),
    /// with only the tab shown listed now; else Home.
    pub(crate) fn open_start_tabs(
        &mut self,
        locations: Vec<Location>,
        session: Option<Session>,
        cx: &mut Context<Self>,
    ) {
        let restored =
            session.filter(|_| locations.is_empty() && self.features.tabs_restore_session);
        if let Some(session) = restored {
            for saved in &session.tabs {
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                let mut tab = crate::tab::Tab::new(id, saved.location());
                tab.unloaded = true;
                self.tabs.push(tab);
            }
            self.active = session.active.min(self.tabs.len().saturating_sub(1));
            self.tab_mut().unloaded = false;
            self.load(cx);
            return;
        }
        let locations = if locations.is_empty() {
            vec![Location::Home]
        } else {
            locations
        };
        for location in locations {
            self.open_tab(location, cx);
        }
        self.active = 0;
    }

    /// Tabs or the window changed: the session is written a moment later.
    pub(crate) fn session_changed(&mut self, cx: &mut Context<Self>) {
        if self.session_file.is_none() || !self.features.tabs_restore_session {
            return;
        }
        self.session_ticket += 1;
        let ticket = self.session_ticket;
        let timer = cx.background_executor().timer(WRITE_AFTER);
        cx.spawn(async move |this, cx| {
            timer.await;
            let job = this
                .update(cx, |this, _| {
                    (this.session_ticket == ticket)
                        .then(|| (this.session_file.clone(), this.session()))
                })
                .ok()
                .flatten();
            let Some((Some(file), session)) = job else {
                return;
            };
            let written = crate::ops::off_thread(move || save(&file, &session)).await;
            if let Some(Err(err)) = written {
                tracing::warn!(error = %err, "session not saved");
            }
        })
        .detach();
    }

    /// Writes the session now (the window is closing; a pending write would be lost).
    pub fn save_session_now(&mut self) {
        let Some(file) = &self.session_file else {
            return;
        };
        if !self.features.tabs_restore_session {
            return;
        }
        // One small local file, as the app quits: nothing else is waiting on this thread.
        if let Err(err) = save(file, &self.session()) {
            tracing::warn!(error = %err, "session not saved");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_and_a_broken_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        assert_eq!(load(&file), None);
        let session = Session {
            tabs: vec![
                SavedTab::Home,
                SavedTab::Folder(PathBuf::from(r"D:\Foto gw")),
                SavedTab::RecycleBin,
            ],
            active: 1,
            window: Some(SavedWindow {
                x: 100.0,
                y: 80.0,
                width: 1200.0,
                height: 800.0,
                maximized: true,
            }),
        };
        save(&file, &session).unwrap();
        assert_eq!(load(&file), Some(session));
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(load(&file), None);
        std::fs::write(&file, br#"{"tabs":[],"active":0}"#).unwrap();
        assert_eq!(load(&file), None, "no tabs: nothing to restore");
    }

    #[test]
    fn a_window_off_every_display_opens_in_the_default_place() {
        let display = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(1920.0), px(1080.0)),
        };
        let place = SavedWindow {
            x: 3000.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
            maximized: false,
        };
        assert_eq!(place.bounds(&[display], (640.0, 400.0)), None);
        let shown = SavedWindow { x: 100.0, ..place };
        assert!(matches!(
            shown.bounds(&[display], (640.0, 400.0)),
            Some(WindowBounds::Windowed(_))
        ));
    }
}
