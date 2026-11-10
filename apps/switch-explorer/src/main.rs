//! Switch Explorer — a fast, customizable file explorer for Windows.
//!
//! Startup (ARCHITECTURE.md §13): arguments → settings → theme → window. Services that are not
//! needed for the first frame (drives, OS theme watch) start from the workspace afterwards.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod args;
mod logging;
mod single;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context as _;
use futures::channel::mpsc;
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, Global, WindowBackgroundAppearance, WindowBounds, WindowOptions, px, size,
};
use sx_core::{CommandRegistry, Location};
use sx_settings::{SettingsError, SettingsStore};
use sx_theme::Mode;
use sx_ui::{OpenRequest, ThemeController, Workspace, WorkspaceOptions};

use crate::args::Args;

/// First-run window size; later runs restore the last one (session restore, Phase 2).
const WINDOW_SIZE: (f32, f32) = (1180.0, 760.0);
/// Below this the layout switches to its narrow form (DESIGN.md §8.8).
const WINDOW_MIN: (f32, f32) = (560.0, 400.0);

/// Keeps the single-instance server running for the life of the app.
struct IpcServer(#[allow(dead_code)] sx_ipc::ServerHandle);

impl Global for IpcServer {}

fn data_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|base| PathBuf::from(base).join("SwitchExplorer"))
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let args = Args::parse()?;
    let (settings, warnings) = load_settings(&args);
    let features = settings.snapshot();
    let level = if args.safe_mode {
        "debug"
    } else {
        features.dev_log_level
    };
    let _log = logging::init(
        data_dir("LOCALAPPDATA").map(|d| d.join("logs")).as_deref(),
        level,
    );
    for warning in &warnings {
        tracing::warn!(error = %warning, "settings");
        tracing::debug!(?warning, "settings");
    }
    let server = match single::claim(&args) {
        single::Role::Forwarded => {
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                "forwarded to the running instance"
            );
            return Ok(());
        }
        single::Role::Primary(server) => server,
    };
    let os_build = sx_win::os_build();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os_build,
        safe_mode = args.safe_mode,
        "start"
    );
    sx_ui::set_locale(
        args.lang
            .as_deref()
            .unwrap_or_else(|| sx_win::ui_language()),
    );
    let registry = CommandRegistry::builtin().context("command registry")?;
    let (requests_tx, requests) = mpsc::unbounded::<OpenRequest>();
    let ipc = server.and_then(|server| single::serve(server, requests_tx));
    gpui_kit::application()
        .with_assets(sx_ui::Assets)
        .run(move |cx| {
            // GPUI platform start-up (DirectX, DirectWrite) is most of the cold start.
            tracing::debug!(ms = started.elapsed().as_secs_f64() * 1000.0, "app running");
            gpui_kit::init(cx);
            // OS modal loops (the shell's menu) run from this thread's host window (ADR-015).
            if let Err(err) = sx_shell::modal::init() {
                tracing::warn!(error = %err, "modal host unavailable: shell menus will not open");
            }
            sx_ui::actions::bind(&registry, cx);
            tracing::debug!(ms = started.elapsed().as_secs_f64() * 1000.0, "kit ready");
            if let Some(handle) = ipc {
                cx.set_global(IpcServer(handle));
            }
            open_main_window(&args, settings, os_build, started, requests, cx);
        });
    Ok(())
}

/// `settings.toml` (defaults in safe mode) plus the session overrides from the command line.
fn load_settings(args: &Args) -> (SettingsStore, Vec<SettingsError>) {
    let (mut settings, mut warnings) = if args.safe_mode {
        (SettingsStore::in_memory(), Vec::new())
    } else {
        match data_dir("APPDATA") {
            Some(dir) => SettingsStore::load(&dir.join("settings.toml"))
                .unwrap_or_else(|err| (SettingsStore::in_memory(), vec![err])),
            None => (SettingsStore::in_memory(), Vec::new()),
        }
    };
    let dev = args.dev.then_some("dev.mode=true");
    for assignment in dev.into_iter().chain(args.sets.iter().map(String::as_str)) {
        if let Err(err) = settings.apply_cli(assignment) {
            warnings.push(err);
        }
    }
    (settings, warnings)
}

fn open_main_window(
    args: &Args,
    settings: SettingsStore,
    os_build: u32,
    started: Instant,
    requests: mpsc::UnboundedReceiver<OpenRequest>,
    cx: &mut App,
) {
    let mut appearance = sx_shell::appearance::current();
    if let Some(light) = args.light {
        appearance.light = light;
    }
    let backdrop_supported = os_build >= sx_win::BUILD_SYSTEM_BACKDROP;
    let mode = if appearance.light {
        Mode::Light
    } else {
        Mode::Dark
    };
    let solid = !(backdrop_supported && appearance.transparency);
    match ThemeController::builtin() {
        Ok(controller) => controller.install(mode, solid, cx),
        Err(err) => tracing::error!(error = %err, "built-in theme failed to parse"),
    }
    let session_file = (!args.safe_mode)
        .then(|| data_dir("LOCALAPPDATA").map(|d| d.join("session.json")))
        .flatten();
    let session = session_file
        .as_deref()
        .filter(|_| settings.snapshot().tabs_restore_session)
        .and_then(sx_ui::session::load);
    let options = WorkspaceOptions {
        locations: start_locations(args),
        settings,
        backdrop_supported,
        appearance,
        forced_light: args.light,
        bench_started: args.bench_open.is_some().then_some(started),
        requests: Some(requests),
        pins_file: (!args.safe_mode)
            .then(|| data_dir("APPDATA").map(|d| d.join("pins.json")))
            .flatten(),
        journal_dir: (!args.safe_mode)
            .then(|| data_dir("LOCALAPPDATA").map(|d| d.join("journal")))
            .flatten(),
        session_file: session_file.clone(),
        session: session.clone(),
    };
    // The last window's place, when it still shows on a display; centered otherwise.
    let displays: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
    let restored_bounds = session
        .as_ref()
        .and_then(|s| s.window)
        .and_then(|w| w.bounds(&displays, WINDOW_MIN));
    let window_options = WindowOptions {
        window_bounds: Some(restored_bounds.unwrap_or_else(|| {
            WindowBounds::centered(size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx)
        })),
        window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
        window_background: if solid {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::MicaAltBackdrop
        },
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(window_options, cx, |window, cx| {
        let workspace = cx.new(|cx| Workspace::new(options, window, cx));
        let weak = workspace.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            // A change from the last moment would otherwise not be written yet.
            let _ = weak.update(cx, |workspace, _| workspace.save_session_now());
            cx.quit();
            true
        });
        workspace
    });
    match opened {
        Ok(_) => tracing::info!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "window opened"
        ),
        Err(err) => {
            tracing::error!(error = %err, "cannot open the main window");
            cx.quit();
        }
    }
}

/// Tabs to open: `--bench-open`, then paths, then `--select`'s folder; Home otherwise. Relative
/// paths are made absolute here, at the boundary, never inside the file system layer.
fn start_locations(args: &Args) -> Vec<Location> {
    let absolute = |p: &PathBuf| std::path::absolute(p).unwrap_or_else(|_| p.clone());
    if let Some(path) = &args.bench_open {
        return vec![Location::Path(absolute(path))];
    }
    let mut out: Vec<Location> = args
        .paths
        .iter()
        .map(|p| Location::Path(absolute(p)))
        .collect();
    if let Some(parent) = args
        .select
        .as_ref()
        .and_then(|p| absolute(p).parent().map(PathBuf::from))
    {
        out.push(Location::Path(parent));
    }
    // Nothing asked for: the last session, or Home (the workspace decides).
    if args.home {
        out.insert(0, Location::Home);
    }
    out
}
