//! Switch Explorer — a fast, customizable file explorer for Windows.
//!
//! Startup (ARCHITECTURE.md §13): arguments → settings → theme → window. Services that are not
//! needed for the first frame (drives, OS theme watch) start from the workspace afterwards.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod args;
mod logging;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, WindowBackgroundAppearance, WindowBounds, WindowOptions, px, size,
};
use sx_core::{CommandRegistry, Location};
use sx_settings::{Features, SettingsError, SettingsStore};
use sx_theme::Mode;
use sx_ui::{ThemeController, Workspace, WorkspaceOptions};

use crate::args::Args;

/// First-run window size; later runs restore the last one (session restore, Phase 2).
const WINDOW_SIZE: (f32, f32) = (1180.0, 760.0);
/// Below this the layout switches to its narrow form (DESIGN.md §8.8).
const WINDOW_MIN: (f32, f32) = (560.0, 400.0);

fn data_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|base| PathBuf::from(base).join("SwitchExplorer"))
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let args = Args::parse()?;
    let (features, warnings) = load_settings(&args);
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
    }
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
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            sx_ui::actions::bind(&registry, cx);
            open_main_window(&args, features, os_build, started, cx);
        });
    Ok(())
}

/// `settings.toml` (defaults in safe mode) plus the session overrides from the command line.
fn load_settings(args: &Args) -> (Arc<Features>, Vec<SettingsError>) {
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
    (settings.snapshot(), warnings)
}

fn open_main_window(
    args: &Args,
    features: Arc<Features>,
    os_build: u32,
    started: Instant,
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
    let options = WorkspaceOptions {
        locations: start_locations(args),
        features,
        backdrop_supported,
        appearance,
        forced_light: args.light,
        bench_started: args.bench_open.is_some().then_some(started),
    };
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)),
            cx,
        )),
        window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
        window_background: if solid {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::MicaAltBackdrop
        },
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(window_options, cx, |window, cx| {
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            true
        });
        cx.new(|cx| Workspace::new(options, window, cx))
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
    if args.home || out.is_empty() {
        out.insert(0, Location::Home);
    }
    out
}
