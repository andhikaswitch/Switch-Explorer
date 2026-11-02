//! The spike workspace: custom title bar over Mica Alt, a 100k-row table (DataTable or
//! uniform_list), Hot Switch chords plus a global hotkey with OSD, OLE drag-out, the native shell
//! menu, drop handling, an IME test input, and the benchmark driver.

use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, Theme, ThemeMode, TitleBar,
};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::Args;
use crate::bench::{self, Bench, FrameLog, Scenario, ScrollTarget};
use crate::data::{self, Dataset, Source, Variant};
use crate::osd;
use crate::table::{FileTable, RowHit};
use crate::win::{self, Hwnd, dnd, hotkey, menu, modal, theme_watch};

pub const ENGINE: &str = if cfg!(feature = "gpui-fast") {
    "gpui-fast"
} else {
    "upstream"
};
pub const PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};

/// Hot Switch chords are only active outside text input (ADR-014). GPUI evaluates `!Input`
/// against the whole focus context stack, so the predicate alone keeps chords out of inputs.
const CHORD_CONTEXT: &str = "Workspace && !Input";
const ROW_HEIGHT: f32 = 28.0;
const DRAG_THRESHOLD: f32 = 4.0;

actions!(spike, [ToggleDragFolders, ToggleDragFiles]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k d", ToggleDragFolders, Some(CHORD_CONTEXT)),
        KeyBinding::new("ctrl-k f", ToggleDragFiles, Some(CHORD_CONTEXT)),
    ]);
}

pub fn open_main_window(args: Args, started: Instant, cx: &mut App) -> anyhow::Result<()> {
    let os_build = win::os_build();
    // GPUI only applies Mica on build >= 22621 and silently does nothing below it, so the
    // fallback to a solid window is decided here (Windows 10 / Windows 11 21H2).
    let backdrop = if args.opaque || os_build < 22_621 {
        WindowBackgroundAppearance::Opaque
    } else {
        WindowBackgroundAppearance::MicaAltBackdrop
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
        window_min_size: Some(size(px(720.), px(420.))),
        window_background: backdrop,
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| Workspace::new(args, started, backdrop, os_build, window, cx))
    })?;
    Ok(())
}

fn backdrop_label(backdrop: WindowBackgroundAppearance) -> &'static str {
    match backdrop {
        WindowBackgroundAppearance::MicaAltBackdrop => "mica-alt",
        WindowBackgroundAppearance::MicaBackdrop => "mica",
        WindowBackgroundAppearance::Blurred => "blurred",
        WindowBackgroundAppearance::Transparent => "transparent",
        WindowBackgroundAppearance::Opaque => "opaque",
    }
}

/// gpui-component's Root paints `theme.background` over the whole window, which hides Mica
/// completely. Make the window chrome transparent so the backdrop shows through. Theme changes
/// reload the theme, so this is re-applied after every light/dark switch.
fn apply_translucency(backdrop: WindowBackgroundAppearance, cx: &mut App) {
    if backdrop == WindowBackgroundAppearance::Opaque {
        return;
    }
    Theme::update(cx, |theme| {
        theme.background = theme.background.opacity(0.);
        theme.title_bar = theme.title_bar.opacity(0.);
    });
}

#[derive(Clone, Copy, Debug)]
struct Switches {
    drag_all: bool,
    drag_files: bool,
    drag_folders: bool,
}

impl Switches {
    fn chip(self) -> &'static str {
        if !self.drag_all || (!self.drag_files && !self.drag_folders) {
            "Drag nonaktif"
        } else if self.drag_files && self.drag_folders {
            "Drag aktif"
        } else {
            "Drag sebagian"
        }
    }
}

struct Osd {
    title: SharedString,
    detail: SharedString,
    id: u64,
}

struct Press {
    at: Point<Pixels>,
    row: Option<usize>,
}

pub struct Workspace {
    args: Rc<Args>,
    started: Instant,
    backdrop: WindowBackgroundAppearance,
    os_build: u32,
    adapter: String,
    hwnd: Option<Hwnd>,
    focus: FocusHandle,
    source: Source,
    variant: Variant,
    fixture: Option<Rc<Dataset>>,
    synthetic: Option<Rc<Dataset>>,
    data: Rc<Dataset>,
    table: Entity<TableState<FileTable>>,
    list_scroll: UniformListScrollHandle,
    hit: RowHit,
    selected: Option<usize>,
    input: Entity<InputState>,
    switches: Switches,
    osd: Option<Osd>,
    osd_seq: u64,
    events: VecDeque<SharedString>,
    press: Option<Press>,
    modal: Option<&'static str>,
    ticks: u64,
    drop_hover: bool,
    drop_shared: Rc<dnd::DropShared>,
    own_drop: Option<dnd::OwnDropTarget>,
    hotkey_status: SharedString,
    theme_t0: Option<Instant>,
    frames: Option<FrameLog>,
    bench: Option<Bench>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    fn new(
        args: Args,
        started: Instant,
        backdrop: WindowBackgroundAppearance,
        os_build: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let args = Rc::new(args);
        let hit = RowHit::default();
        let data = Rc::new(Dataset::default());
        let table =
            cx.new(|cx| TableState::new(FileTable::new(data.clone(), hit.clone()), window, cx));
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Uji IME di sini — Ctrl+K D tidak boleh aktif saat mengetik")
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let subscriptions =
            vec![cx.observe_window_appearance(window, |this, window, cx| {
                this.on_appearance(window, cx)
            })];
        Theme::sync_system_appearance(Some(window), cx);
        apply_translucency(backdrop, cx);

        // The bench (and its profiler collector) must exist before the first frame is drawn.
        let bench = args
            .bench
            .map(|scenario| Bench::new(scenario, args.seconds));
        let frames = (args.trace && bench.is_none()).then(FrameLog::new);

        let mut this = Self {
            source: args.source,
            variant: args.variant,
            args,
            started,
            backdrop,
            os_build,
            adapter: win::primary_adapter(),
            hwnd: win::hwnd(window),
            focus,
            fixture: None,
            synthetic: None,
            data,
            table,
            list_scroll: UniformListScrollHandle::new(),
            hit,
            selected: None,
            input,
            switches: Switches {
                drag_all: true,
                drag_files: true,
                drag_folders: true,
            },
            osd: None,
            osd_seq: 0,
            events: VecDeque::new(),
            press: None,
            modal: None,
            ticks: 0,
            drop_hover: false,
            drop_shared: Rc::new(dnd::DropShared::default()),
            own_drop: None,
            hotkey_status: SharedString::from("hotkey global: mendaftar…"),
            theme_t0: None,
            frames,
            bench,
            _subscriptions: subscriptions,
        };
        this.push_event(format!(
            "backdrop {} · build {} · GPU {} · {} {}",
            backdrop_label(backdrop),
            os_build,
            this.adapter,
            ENGINE,
            PROFILE
        ));
        this.start_services(window, cx);
        this
    }

    fn start_services(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Global hotkey thread → UI.
        let (tx, mut rx) = mpsc::unbounded();
        hotkey::spawn(tx);
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = rx.next().await {
                let applied =
                    this.update_in(cx, |this, window, cx| this.on_hotkey(event, window, cx));
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();

        // Registry watcher: switches the theme as soon as Windows writes AppsUseLightTheme, instead
        // of waiting for the WM_SETTINGCHANGE broadcast (measured ~600 ms late on a busy desktop).
        let (tx, mut rx) = mpsc::unbounded();
        theme_watch::spawn(tx);
        cx.spawn_in(window, async move |this, cx| {
            while let Some(theme_watch::ThemeEvent::Changed { light, at }) = rx.next().await {
                let applied = this.update_in(cx, |this, window, cx| {
                    this.on_registry_theme(light, at, window, cx)
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();

        // Own drop target (Phase 0 decision point); GPUI's built-in one is the default.
        if self.args.own_drop_target {
            self.install_own_drop_target(cx);
        }

        self.load_source(self.source, cx);

        if self.frames.is_some() {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let polled = this.update(cx, |this, _| {
                        if let Some(frames) = this.frames.as_mut() {
                            frames.poll();
                        }
                    });
                    if polled.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }

        match self.args.bench {
            Some(Scenario::Startup) => self.drive_startup(cx),
            Some(Scenario::Idle) => self.drive_idle(cx),
            Some(_) => self.schedule_bench_frame(window, cx),
            None => {}
        }
    }

    fn install_own_drop_target(&mut self, cx: &mut Context<Self>) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let (tx, mut rx) = mpsc::unbounded();
        match dnd::install(hwnd, self.drop_shared.clone(), tx) {
            Ok(target) => {
                self.own_drop = Some(target);
                self.push_event("drop target: milik sendiri (RevokeDragDrop + RegisterDragDrop)");
            }
            Err(err) => self.push_event(format!("drop target sendiri gagal: {err}")),
        }
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                if this
                    .update(cx, |this, cx| this.on_drop_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn load_source(&mut self, source: Source, cx: &mut Context<Self>) {
        self.source = source;
        let cached = match source {
            Source::Fixture => self.fixture.clone(),
            Source::Synthetic => self.synthetic.clone(),
        };
        if let Some(data) = cached {
            self.set_data(data, cx);
            return;
        }
        let rows = self.args.rows;
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    match source {
                        Source::Synthetic => Ok(Dataset::synthetic(rows)),
                        Source::Fixture => {
                            data::ensure_fixture().and_then(|dir| Dataset::load_dir(&dir))
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok(data) => {
                    let data = Rc::new(data);
                    this.push_event(format!(
                        "data {}: {} baris dalam {:.0} ms ({:.1} MB)",
                        source.name(),
                        data.len(),
                        bench::ms(started.elapsed()),
                        data.heap_bytes() as f64 / 1_048_576.0
                    ));
                    match source {
                        Source::Fixture => this.fixture = Some(data.clone()),
                        Source::Synthetic => this.synthetic = Some(data.clone()),
                    }
                    if this.source == source {
                        this.set_data(data, cx);
                    }
                }
                Err(err) => {
                    this.push_event(format!("gagal memuat data {}: {err}", source.name()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_data(&mut self, data: Rc<Dataset>, cx: &mut Context<Self>) {
        self.data = data.clone();
        self.selected = None;
        self.hit.set(None);
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_data(data);
            table.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    fn push_event(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::info!(target: "sx_spike", "{text}");
        self.events.push_front(SharedString::from(text));
        self.events.truncate(5);
    }

    // ---- Hot Switch & OSD -------------------------------------------------------------------

    fn on_toggle_folders(
        &mut self,
        _: &ToggleDragFolders,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = Instant::now();
        self.switches.drag_folders = !self.switches.drag_folders;
        let (title, detail) = if self.switches.drag_folders {
            ("Drag folder diaktifkan", "Folder bisa di-drag lagi.")
        } else if self.switches.drag_files {
            ("Drag folder dinonaktifkan", "Drag file tetap aktif.")
        } else {
            ("Drag folder dinonaktifkan", "Drag file juga nonaktif.")
        };
        self.show_osd(title, detail, at, window, cx);
    }

    fn on_toggle_files(
        &mut self,
        _: &ToggleDragFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = Instant::now();
        self.switches.drag_files = !self.switches.drag_files;
        let (title, detail) = if self.switches.drag_files {
            ("Drag file diaktifkan", "File bisa di-drag lagi.")
        } else if self.switches.drag_folders {
            ("Drag file dinonaktifkan", "Drag folder tetap aktif.")
        } else {
            ("Drag file dinonaktifkan", "Drag folder juga nonaktif.")
        };
        self.show_osd(title, detail, at, window, cx);
    }

    fn on_hotkey(
        &mut self,
        event: hotkey::HotkeyEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            hotkey::HotkeyEvent::Registered(combo) => {
                self.hotkey_status = SharedString::from(format!("{combo} global aktif"));
            }
            hotkey::HotkeyEvent::Failed { combo, error } => {
                self.hotkey_status = SharedString::from(format!("{combo} gagal"));
                self.push_event(format!(
                    "RegisterHotKey {combo} gagal (sudah dipakai aplikasi lain?): {error}"
                ));
            }
            hotkey::HotkeyEvent::Fired { at } => {
                self.switches.drag_all = !self.switches.drag_all;
                let (title, detail) = if self.switches.drag_all {
                    (
                        "Drag & drop diaktifkan",
                        "Semua jenis item bisa di-drag lagi.",
                    )
                } else {
                    (
                        "Drag & drop dinonaktifkan",
                        "Tekan Ctrl+Shift+D lagi untuk mengaktifkan.",
                    )
                };
                if window.is_window_active() {
                    self.show_osd(title, detail, at, window, cx);
                } else {
                    self.push_event(format!("hotkey global saat app di background: {title}"));
                    if let Err(err) = osd::open_global(title.into(), detail.into(), at, cx) {
                        self.push_event(format!("OSD global gagal: {err}"));
                    }
                }
            }
        }
        cx.notify();
    }

    fn show_osd(
        &mut self,
        title: &str,
        detail: &str,
        requested: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.osd_seq += 1;
        let id = self.osd_seq;
        self.osd = Some(Osd {
            title: SharedString::from(title.to_string()),
            detail: SharedString::from(detail.to_string()),
            id,
        });
        self.push_event(format!("OSD: {title} — {detail}"));
        cx.notify();
        self.probe_latency("osd", requested, window, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1600))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.osd.as_ref().is_some_and(|osd| osd.id == id) {
                    this.osd = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Logs input → present latency (profiler, with `--trace`) or input → next frame start.
    fn probe_latency(
        &mut self,
        label: &'static str,
        start: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.frames.is_none() {
            window.on_next_frame(move |_, _| {
                log::info!(
                    target: "sx_spike",
                    "latency {label}: {:.1} ms (input → next frame start, approx; use --trace for present)",
                    bench::ms(start.elapsed())
                );
            });
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |this, _| {
                let Some(frames) = this.frames.as_mut() else {
                    return;
                };
                frames.poll();
                match frames.first_present_after(start) {
                    Some(present) => log::info!(
                        target: "sx_spike",
                        "latency {label}: {:.1} ms (input → present)",
                        bench::ms(present.duration_since(start))
                    ),
                    None => log::info!(target: "sx_spike", "latency {label}: no present seen"),
                }
            });
        })
        .detach();
    }

    // ---- Light/dark -------------------------------------------------------------------------

    fn on_registry_theme(
        &mut self,
        light: bool,
        at: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_t0 = Some(at);
        if let Some(hwnd) = self.hwnd {
            win::set_dark_frame(hwnd, !light);
        }
        let mode = if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        Theme::change(mode, Some(window), cx);
        apply_translucency(self.backdrop, cx);
        self.push_event(format!(
            "registry tema → {} (diterapkan sebelum WM_SETTINGCHANGE)",
            if light { "light" } else { "dark" }
        ));
        cx.notify();
        self.probe_latency("theme-registry", at, window, cx);
    }

    fn on_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t1 = Instant::now();
        Theme::sync_system_appearance(Some(window), cx);
        apply_translucency(self.backdrop, cx);
        let dark = cx.theme().is_dark();
        self.push_event(format!(
            "tema sistem berubah → {}",
            if dark { "dark" } else { "light" }
        ));
        cx.notify();
        // Report once the new theme has been presented and the registry event has arrived.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, _| {
                let t0 = this
                    .theme_t0
                    .filter(|t0| t1.saturating_duration_since(*t0) < Duration::from_secs(5));
                let present = this.frames.as_mut().and_then(|frames| {
                    frames.poll();
                    frames.first_present_after(t1)
                });
                let fmt = |d: Option<Duration>| d.map_or("n/a".to_string(), |d| format!("{:.1} ms", bench::ms(d)));
                log::info!(
                    target: "sx_spike",
                    "latency theme: registry→callback {} | callback→present {} | registry→present {}",
                    fmt(t0.map(|t0| t1.saturating_duration_since(t0))),
                    fmt(present.map(|p| p.saturating_duration_since(t1))),
                    fmt(t0.zip(present).map(|(t0, p)| p.saturating_duration_since(t0)))
                );
            });
        })
        .detach();
    }

    // ---- Modal loops: OLE drag-out and native menu ------------------------------------------

    /// Marks a modal OS loop as running and ticks every 100 ms from a foreground task. If GPUI's
    /// executor keeps running inside the nested loop, ticks ≈ duration / 100 ms.
    fn enter_modal(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        self.modal = Some(kind);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        if this.modal.is_some() {
                            this.ticks += 1;
                            log::info!(target: "sx_spike", "modal tick {}", this.ticks);
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_content_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let row = self.hit.get();
        self.selected = row;
        self.press = Some(Press {
            at: event.position,
            row,
        });
    }

    fn on_content_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal.is_some() || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(press) = &self.press else {
            return;
        };
        let delta = event.position - press.at;
        if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < DRAG_THRESHOLD {
            return;
        }
        let row = press.row;
        self.press = None;
        if let Some(row) = row {
            self.begin_drag(row, window, cx);
        }
    }

    fn begin_drag(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let is_dir = self.data.is_dir(row);
        let blocked = if !self.switches.drag_all {
            Some("Tekan Ctrl+Shift+D untuk mengaktifkan drag lagi.")
        } else if is_dir && !self.switches.drag_folders {
            Some("Drag folder nonaktif. Tekan Ctrl+K lalu D untuk mengaktifkan.")
        } else if !is_dir && !self.switches.drag_files {
            Some("Drag file nonaktif. Tekan Ctrl+K lalu F untuk mengaktifkan.")
        } else {
            None
        };
        if let Some(hint) = blocked {
            self.show_osd("Drag diblokir", hint, Instant::now(), window, cx);
            return;
        }
        let (Some(path), Some(hwnd)) = (self.data.path(row), self.hwnd) else {
            self.push_event(
                "baris sintetis tidak punya file nyata — pakai 'Folder fixture' untuk uji drag",
            );
            cx.notify();
            return;
        };
        self.enter_modal("drag", cx);
        let started = Instant::now();
        let ticks_before = self.ticks;
        // DoDragDrop runs from the modal host window (outside GPUI's borrows and executor); its
        // nested message loop can re-enter GPUI (paint, input, tasks) freely.
        let result = modal::run(move || {
            let result = dnd::start_drag(hwnd, std::slice::from_ref(&path), dnd::SPIKE_ALLOWED);
            dnd::post_button_up(hwnd);
            result.map_err(|err| err.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = result.await;
            let elapsed = started.elapsed();
            let _ = this.update(cx, |this, cx| {
                this.modal = None;
                let ticks = this.ticks - ticks_before;
                this.push_event(match result {
                    Ok(Ok(effect)) => format!(
                        "drag-out selesai: {} dalam {:.1} s · tick UI {ticks}/{} (loop tidak beku jika mendekati)",
                        dnd::effect_name(effect),
                        elapsed.as_secs_f32(),
                        elapsed.as_millis() / 100
                    ),
                    Ok(Err(err)) => format!("drag-out gagal: {err}"),
                    Err(_) => String::from("drag-out dibatalkan (job modal tidak berjalan)"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn on_content_right_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal.is_some() {
            return;
        }
        let Some(row) = self.hit.get() else {
            return;
        };
        self.selected = Some(row);
        let (Some(path), Some(hwnd)) = (self.data.path(row), self.hwnd) else {
            self.push_event(
                "baris sintetis tidak punya file nyata — pakai 'Folder fixture' untuk menu native",
            );
            cx.notify();
            return;
        };
        let scale = window.scale_factor();
        let client = (
            (f32::from(event.position.x) * scale).round() as i32,
            (f32::from(event.position.y) * scale).round() as i32,
        );
        let extended = event.modifiers.shift;
        let dark = cx.theme().is_dark();
        self.enter_modal("menu", cx);
        let started = Instant::now();
        let ticks_before = self.ticks;
        let outcome = modal::run(move || {
            menu::show(hwnd, &path, client, extended, dark).map_err(|err| err.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = outcome.await;
            let elapsed = started.elapsed();
            let _ = this.update(cx, |this, cx| {
                this.modal = None;
                let ticks = this.ticks - ticks_before;
                this.push_event(match outcome {
                    Ok(Ok(outcome)) => format!(
                        "menu native: {outcome} · tick UI {ticks}/{}",
                        elapsed.as_millis() / 100
                    ),
                    Ok(Err(err)) => format!("menu native gagal: {err}"),
                    Err(_) => String::from("menu native dibatalkan (job modal tidak berjalan)"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn on_builtin_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let list = paths.paths();
        let example = list
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.push_event(format!(
            "drop (bawaan GPUI, selalu COPY): {} path, contoh {example}",
            list.len()
        ));
        cx.notify();
    }

    fn on_drop_event(&mut self, event: dnd::DropEvent, cx: &mut Context<Self>) {
        match event {
            dnd::DropEvent::Enter {
                files,
                effect,
                in_zone,
                formats,
            } => {
                self.drop_hover = in_zone;
                self.push_event(format!(
                    "drag masuk: {files} file, efek {}, format sumber: {}",
                    dnd::effect_name(effect),
                    formats.join(", ")
                ));
            }
            dnd::DropEvent::Over { in_zone } => self.drop_hover = in_zone,
            dnd::DropEvent::Leave => self.drop_hover = false,
            dnd::DropEvent::Drop {
                paths,
                effect,
                in_zone,
            } => {
                self.drop_hover = false;
                let example = paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.push_event(format!(
                    "drop (target sendiri): {} path, efek {}{} — contoh {example}",
                    paths.len(),
                    dnd::effect_name(effect),
                    if in_zone {
                        ""
                    } else {
                        ", di luar zona (ditolak)"
                    }
                ));
            }
        }
        cx.notify();
    }

    // ---- Benchmarks ---------------------------------------------------------------------------

    fn run_info(&self) -> bench::RunInfo {
        bench::RunInfo {
            tag: self.args.tag.clone(),
            variant: self.variant.name(),
            rows: self.data.len(),
            engine: ENGINE,
            profile: PROFILE,
            adapter: self.adapter.clone(),
            backdrop: backdrop_label(self.backdrop),
            private_ws_mb: win::private_working_set().map_or(f64::NAN, |b| b as f64 / 1_048_576.0),
            os_build: self.os_build,
        }
    }

    /// Runs `bench_frame` on the next frame. Continuous frames come from `render`, which calls
    /// `request_animation_frame` while a scroll bench runs (GPUI only allows it during drawing).
    fn schedule_bench_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = this.update(cx, |this, cx| this.bench_frame(window, cx));
        });
        cx.notify();
    }

    /// One step of a scroll scenario: move the list to where it should be "now".
    fn bench_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut bench) = self.bench.take() else {
            return;
        };
        let now = Instant::now();
        if !bench.started() && self.data.len() > 0 && self.source == Source::Synthetic {
            bench.begin(now);
        }
        if let Some(target) = bench.target(now, self.data.len()) {
            match (self.variant, target) {
                (Variant::DataTable, ScrollTarget::Row(row)) => {
                    self.table
                        .update(cx, |table, cx| table.scroll_to_row(row, cx));
                }
                (Variant::DataTable, ScrollTarget::Pixel(y)) => {
                    let row = (y / ROW_HEIGHT) as usize;
                    self.table
                        .update(cx, |table, cx| table.scroll_to_row(row, cx));
                }
                (Variant::Uniform, ScrollTarget::Row(row)) => {
                    self.list_scroll
                        .scroll_to_item_strict(row, ScrollStrategy::Top);
                }
                (Variant::Uniform, ScrollTarget::Pixel(y)) => {
                    self.list_scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(-y)));
                }
            }
            cx.notify();
        }
        bench.collect();
        if bench.is_finished(now) {
            let info = self.run_info();
            match bench.write(&info, &self.args.out_dir) {
                Ok(line) => log::info!(target: "sx_spike", "bench done: {}", line.trim_end()),
                Err(err) => log::error!(target: "sx_spike", "bench write failed: {err}"),
            }
            cx.quit();
            return;
        }
        self.bench = Some(bench);
        self.schedule_bench_frame(window, cx);
    }

    /// Idle: no input for warm-up + N seconds, then record frames drawn and private working set.
    fn drive_idle(&mut self, cx: &mut Context<Self>) {
        let Some(bench) = self.bench.as_mut() else {
            return;
        };
        bench.begin(Instant::now());
        let wait = bench::WARMUP + Duration::from_secs_f32(self.args.seconds);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                let info = this.run_info();
                if let Some(bench) = this.bench.as_mut() {
                    bench.collect();
                    match bench.write(&info, &this.args.out_dir) {
                        Ok(line) => {
                            log::info!(target: "sx_spike", "idle done: {}", line.trim_end())
                        }
                        Err(err) => log::error!(target: "sx_spike", "idle write failed: {err}"),
                    }
                }
                cx.quit();
            });
        })
        .detach();
    }

    /// Startup: time from process creation (and from `main`) to the first presented frame.
    fn drive_startup(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..500 {
                cx.background_executor()
                    .timer(Duration::from_millis(5))
                    .await;
                let done = this
                    .update(cx, |this, cx| this.report_startup(cx))
                    .unwrap_or(true);
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    fn report_startup(&mut self, cx: &mut Context<Self>) -> bool {
        let started = self.started;
        let Some(bench) = self.bench.as_mut() else {
            return true;
        };
        bench.frame_log().poll();
        let Some(first) = bench.frame_log().first_present_after(started) else {
            return false;
        };
        let now = Instant::now();
        let since_main = first.duration_since(started);
        let since_process =
            win::process_age().map(|age| age.saturating_sub(now.duration_since(first)));
        let info = self.run_info();
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\n",
            bench::unix_stamp(),
            info.tag,
            info.engine,
            info.profile,
            info.backdrop,
            info.adapter,
            since_process.map_or(f64::NAN, bench::ms),
            bench::ms(since_main),
            info.private_ws_mb
        );
        let written = bench::append_line(
            &self.args.out_dir.join("startup.tsv"),
            "stamp\ttag\tengine\tprofile\tbackdrop\tadapter\tprocess_to_first_present_ms\tmain_to_first_present_ms\tprivate_ws_mb\n",
            &line,
        );
        if let Err(err) = written {
            log::error!(target: "sx_spike", "startup write failed: {err}");
        }
        log::info!(target: "sx_spike", "startup done: {}", line.trim_end());
        cx.quit();
        true
    }

    // ---- Rendering ----------------------------------------------------------------------------

    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let chip_bg = if self.switches.chip() == "Drag aktif" {
            theme.success
        } else {
            theme.warning
        };
        TitleBar::new().child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Switch Explorer · spike Fase 0"),
                )
                .child(
                    div()
                        .px_2()
                        .rounded_md()
                        .text_xs()
                        .text_color(theme.foreground)
                        .bg(chip_bg.opacity(0.35))
                        .border_1()
                        .border_color(chip_bg)
                        .child(self.switches.chip()),
                ),
        )
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .items_center()
            .child(
                Button::new("src-fixture")
                    .small()
                    .label("Folder fixture")
                    .selected(self.source == Source::Fixture)
                    .on_click(cx.listener(|this, _, _, cx| this.load_source(Source::Fixture, cx))),
            )
            .child(
                Button::new("src-synthetic")
                    .small()
                    .label(format!("{}k sintetis", self.args.rows / 1000))
                    .selected(self.source == Source::Synthetic)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.load_source(Source::Synthetic, cx)),
                    ),
            )
            .child(div().w(px(16.)))
            .child(
                Button::new("variant-table")
                    .small()
                    .label("DataTable")
                    .selected(self.variant == Variant::DataTable)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.variant = Variant::DataTable;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("variant-uniform")
                    .small()
                    .label("uniform_list")
                    .selected(self.variant == Variant::Uniform)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.variant = Variant::Uniform;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(div().w(px(420.)).child(Input::new(&self.input)))
    }

    fn render_content(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (surface, border, accent) = (
            theme.table,
            if self.drop_hover {
                theme.primary
            } else {
                theme.border
            },
            theme.primary,
        );
        let body = match self.variant {
            Variant::DataTable => DataTable::new(&self.table).into_any_element(),
            Variant::Uniform => self.render_uniform(cx).into_any_element(),
        };
        let shared = self.drop_shared.clone();
        div()
            .id("content")
            .relative()
            .flex_1()
            .min_h_0()
            .mx_2()
            .rounded_md()
            .overflow_hidden()
            .bg(surface)
            .border_1()
            .border_color(border)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_content_mouse_down))
            .on_mouse_move(cx.listener(Self::on_content_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.press = None),
            )
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_content_right_up))
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.on_builtin_drop(paths, cx)),
            )
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent))
            .child(body)
            .child(
                canvas(
                    move |bounds, _, _| shared.zone.set(Some(dnd::Zone::from_bounds(bounds))),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
    }

    fn render_uniform(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let header = h_flex()
            .h(px(ROW_HEIGHT))
            .px_3()
            .gap_3()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().child("Nama"))
            .child(div().w(px(150.)).child("Diubah"))
            .child(div().w(px(110.)).child("Tipe"))
            .child(div().w(px(100.)).text_right().child("Ukuran"));
        v_flex().size_full().child(header).child(
            uniform_list("rows", self.data.len(), cx.processor(Self::render_rows))
                .track_scroll(&self.list_scroll)
                .flex_1(),
        )
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let (hover, active, folder, file) = (
            theme.list_hover,
            theme.list_active,
            theme.warning,
            theme.info,
        );
        range
            .map(|ix| {
                let cells = self.data.cells(ix);
                let (left, right) = (self.hit.clone(), self.hit.clone());
                let marker = if self.data.is_dir(ix) { folder } else { file };
                h_flex()
                    .id(("u", ix))
                    .h(px(ROW_HEIGHT))
                    .px_3()
                    .gap_3()
                    .items_center()
                    .text_sm()
                    .when(self.selected == Some(ix), |row| row.bg(active))
                    .hover(move |row| row.bg(hover))
                    .on_mouse_down(MouseButton::Left, move |_, _, _| left.set(Some(ix)))
                    .on_mouse_down(MouseButton::Right, move |_, _, _| right.set(Some(ix)))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .overflow_hidden()
                            .child(div().size(px(12.)).flex_none().rounded_sm().bg(marker))
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(cells[0].clone()),
                            ),
                    )
                    .child(div().w(px(150.)).child(cells[1].clone()))
                    .child(div().w(px(110.)).child(cells[2].clone()))
                    .child(div().w(px(100.)).text_right().child(cells[3].clone()))
                    .into_any_element()
            })
            .collect()
    }

    fn render_status(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let spinner = self.modal.map(|kind| {
            const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
            SharedString::from(format!("{kind} {}", FRAMES[(self.ticks % 4) as usize]))
        });
        v_flex()
            .px_3()
            .py_1()
            .gap_0p5()
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(self.events.iter().take(3).map(|event| {
                div()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(event.clone())
            }))
            .child(
                h_flex()
                    .gap_4()
                    .text_color(theme.foreground)
                    .child(SharedString::from(format!("{} baris", self.data.len())))
                    .child(self.switches.chip())
                    .child(self.hotkey_status.clone())
                    .child(if self.own_drop.is_some() {
                        "drop: target sendiri"
                    } else {
                        "drop: bawaan GPUI"
                    })
                    .child(SharedString::from(format!(
                        "{} · build {}",
                        backdrop_label(self.backdrop),
                        self.os_build
                    )))
                    .children(spinner),
            )
    }

    fn render_osd(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let osd = self.osd.as_ref()?;
        let theme = cx.theme();
        Some(
            div()
                .absolute()
                .bottom(px(72.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    v_flex()
                        .px_4()
                        .py_3()
                        .gap_1()
                        .rounded_lg()
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .border_1()
                        .border_color(theme.border)
                        .shadow_lg()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(osd.title.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(osd.detail.clone()),
                        ),
                ),
        )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drop_shared.scale.set(window.scale_factor());
        if self.args.bench.is_some_and(Scenario::is_scroll) {
            window.request_animation_frame();
        }
        v_flex()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .on_action(cx.listener(Self::on_toggle_folders))
            .on_action(cx.listener(Self::on_toggle_files))
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(self.render_content(cx))
            .child(self.render_status(cx))
            .children(self.render_osd(cx))
    }
}
