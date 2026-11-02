//! The flag registry. Every switch is declared once here (DEVELOPER-MODE.md §4, §5.2, §6.3, §7);
//! `define_flags!` generates the typed `Features` struct, the `FLAGS` metadata table and access
//! by id. Adding a switch: see DEVELOPER-MODE.md §12 (skill `/add-feature-flag`).

use std::path::PathBuf;

use crate::error::SettingsError;
use crate::meta::{FlagMeta, Group, Scope, Stability, Visibility};
use crate::value::{FlagField, FlagKind, FlagValue};

macro_rules! flag_type {
    (bool) => { bool };
    (u8) => { u8 };
    (u16) => { u16 };
    (u32) => { u32 };
    (keys) => { String };
    (text) => { String };
    (paths) => { Vec<PathBuf> };
    (choice) => { &'static str };
}

macro_rules! flag_kind {
    (bool) => { FlagKind::Bool };
    (u8) => { FlagKind::Int { min: 0, max: u8::MAX as u32 } };
    (u16) => { FlagKind::Int { min: 0, max: u16::MAX as u32 } };
    (u32) => { FlagKind::Int { min: 0, max: u32::MAX } };
    (u8 [$lo:literal ..= $hi:literal]) => { FlagKind::Int { min: $lo, max: $hi } };
    (u16 [$lo:literal ..= $hi:literal]) => { FlagKind::Int { min: $lo, max: $hi } };
    (u32 [$lo:literal ..= $hi:literal]) => { FlagKind::Int { min: $lo, max: $hi } };
    (keys) => { FlagKind::Keys };
    (text) => { FlagKind::Text };
    (paths) => { FlagKind::Paths };
    (choice ($($var:literal),+)) => { FlagKind::Enum(&[$($var),+]) };
}

macro_rules! or_false {
    () => {
        false
    };
    ($value:literal) => {
        $value
    };
}

macro_rules! define_flags {
    (
        $(
            $(#[doc = $doc:literal])*
            $field:ident : $ty:ident $( ( $($var:literal),+ ) )? $( [ $lo:literal ..= $hi:literal ] )?
                = $default:expr,
                id = $id:literal, group = $group:ident, vis = $vis:ident, stab = $stab:ident,
                scope = $scope:ident
                $(, hot = $hot:literal)? $(, restart = $restart:literal)?
                $(, deps = [ $($dep:literal),* ])?;
        )*
    ) => {
        /// Typed snapshot of every switch. Read it through `SettingsStore::snapshot`.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct Features {
            $( $(#[doc = $doc])* pub $field: flag_type!($ty), )*
        }

        impl Default for Features {
            fn default() -> Self {
                Self { $( $field: $default, )* }
            }
        }

        /// Metadata of every switch, in declaration order.
        pub static FLAGS: &[FlagMeta] = &[
            $(
                FlagMeta {
                    id: $id,
                    field: stringify!($field),
                    kind: flag_kind!($ty $( ($($var),+) )? $( [$lo ..= $hi] )?),
                    group: Group::$group,
                    visibility: Visibility::$vis,
                    stability: Stability::$stab,
                    scope: Scope::$scope,
                    hotswitchable: or_false!($($hot)?),
                    restart: or_false!($($restart)?),
                    depends_on: &[$($($dep),*)?],
                    doc: concat!($($doc),*),
                },
            )*
        ];

        impl Features {
            /// The value of a switch by id.
            pub fn get(&self, id: &str) -> Option<FlagValue> {
                match id {
                    $( $id => Some(FlagField::to_value(&self.$field)), )*
                    _ => None,
                }
            }

            /// Sets a switch by id, validating type, range and variants.
            pub fn set(&mut self, id: &str, value: &FlagValue) -> Result<(), SettingsError> {
                match id {
                    $(
                        $id => {
                            let kind = flag_kind!($ty $( ($($var),+) )? $( [$lo ..= $hi] )?);
                            let value = kind.check($id, value.clone())?;
                            self.$field = FlagField::from_value(&value, kind).ok_or_else(|| {
                                SettingsError::InvalidValue {
                                    id: $id.to_string(),
                                    value: value.to_string(),
                                    expected: kind.describe(),
                                }
                            })?;
                            Ok(())
                        }
                    )*
                    _ => Err(SettingsError::UnknownFlag(id.to_string())),
                }
            }
        }
    };
}

define_flags! {
    // ---- Drag & drop (DEVELOPER-MODE.md §5.2) ------------------------------------------------
    /// Master switch for all drag & drop, including reordering pinned items.
    dnd_enabled: bool = true,
        id = "dnd.enabled", group = Dnd, vis = Basic, stab = Stable, scope = Location, hot = true;
    /// Files may be dragged, from anywhere.
    dnd_source_files: bool = true,
        id = "dnd.source.files", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true, deps = ["dnd.enabled"];
    /// Folders may be dragged (content, sidebar tree, pinned).
    dnd_source_folders: bool = true,
        id = "dnd.source.folders", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true, deps = ["dnd.enabled"];
    /// Sidebar items (pinned and tree) may be dragged out of the sidebar.
    dnd_source_sidebar: bool = true,
        id = "dnd.source.sidebar", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true, deps = ["dnd.enabled"];
    /// Drags to other applications.
    dnd_external_out: bool = true,
        id = "dnd.external_out", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true, deps = ["dnd.enabled"];
    /// Accept drops from other applications.
    dnd_external_in: bool = true,
        id = "dnd.external_in", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true, deps = ["dnd.enabled"];
    /// Allow dropping onto sidebar items.
    dnd_target_sidebar: bool = true,
        id = "dnd.target.sidebar", group = Dnd, vis = Advanced, stab = Stable, scope = Location,
        deps = ["dnd.enabled"];
    /// Allow dropping onto folders in the list.
    dnd_target_folders: bool = true,
        id = "dnd.target.folders", group = Dnd, vis = Advanced, stab = Stable, scope = Location,
        deps = ["dnd.enabled"];
    /// Minimum distance (px at 100% scale) before a drag starts; 0 follows the system.
    dnd_threshold_px: u16[0..=64] = 0,
        id = "dnd.threshold_px", group = Dnd, vis = Advanced, stab = Stable, scope = Location;
    /// The button must be held this long before a drag may start.
    dnd_hold_delay_ms: u16[0..=2000] = 0,
        id = "dnd.hold_delay_ms", group = Dnd, vis = Advanced, stab = Stable, scope = Location;
    /// Drags are ignored for this long after the last scroll.
    dnd_scroll_cooldown_ms: u16[0..=2000] = 0,
        id = "dnd.scroll_cooldown_ms", group = Dnd, vis = Advanced, stab = Stable, scope = Location;
    /// Drag only while this modifier is held at the start.
    dnd_require_modifier: choice("none", "alt", "ctrl", "shift") = "none",
        id = "dnd.require_modifier", group = Dnd, vis = Advanced, stab = Stable, scope = Location;
    /// Right-button drags stay allowed even when the source is switched off (always confirms).
    dnd_right_drag_bypass: bool = false,
        id = "dnd.right_drag_bypass", group = Dnd, vis = Advanced, stab = Stable, scope = Location;
    /// Drop action without modifiers.
    dnd_default_action: choice("system", "copy", "move", "ask") = "system",
        id = "dnd.default_action", group = Dnd, vis = Basic, stab = Stable, scope = Location,
        hot = true;
    /// Confirm when moving a folder by drag.
    dnd_confirm_folder_move: bool = false,
        id = "dnd.confirm_folder_move", group = Dnd, vis = Basic, stab = Stable, scope = Location;
    /// "Undo" toast for 10 seconds after a drop.
    dnd_undo_toast: bool = true,
        id = "dnd.undo_toast", group = Dnd, vis = Basic, stab = Stable, scope = Global;
    /// Tell the user when a switch blocked a drag.
    dnd_blocked_hint: bool = true,
        id = "dnd.blocked_hint", group = Dnd, vis = Basic, stab = Stable, scope = Global;
    /// Hovering a folder during a drag opens it.
    dnd_spring_open: bool = true,
        id = "dnd.spring_open", group = Dnd, vis = Basic, stab = Stable, scope = Location;
    /// Spring-open delay.
    dnd_spring_delay_ms: u16[100..=5000] = 800,
        id = "dnd.spring_delay_ms", group = Dnd, vis = Advanced, stab = Stable, scope = Global;
    /// Order and content of pinned items are locked.
    sidebar_pins_locked: bool = false,
        id = "sidebar.pins_locked", group = Sidebar, vis = Basic, stab = Stable, scope = Global,
        hot = true;

    // ---- Hot Switch (§6.3) -------------------------------------------------------------------
    /// All Hot Switch hotkeys are active.
    hotswitch_enabled: bool = true,
        id = "hotswitch.enabled", group = HotSwitch, vis = Basic, stab = Stable, scope = Global;
    /// Hotkey changes are saved; off = this session only.
    hotswitch_persist: bool = true,
        id = "hotswitch.persist", group = HotSwitch, vis = Basic, stab = Stable, scope = Global;
    /// How switch changes are shown (the status chip always updates).
    hotswitch_osd: choice("in_app", "system", "off") = "in_app",
        id = "hotswitch.osd", group = HotSwitch, vis = Basic, stab = Stable, scope = Global;
    /// How long the OSD stays visible.
    hotswitch_osd_ms: u16[300..=10000] = 1800,
        id = "hotswitch.osd_ms", group = HotSwitch, vis = Advanced, stab = Stable, scope = Global;
    /// Where the OSD appears.
    hotswitch_osd_position: choice("bottom_center", "top_center", "near_cursor") = "bottom_center",
        id = "hotswitch.osd_position", group = HotSwitch, vis = Advanced, stab = Stable,
        scope = Global;
    /// "Drag: …" chip in the status bar.
    hotswitch_status_chip: bool = true,
        id = "hotswitch.status_chip", group = HotSwitch, vis = Basic, stab = Stable, scope = Global;
    /// "Drag off" badge in the title bar while the master switch is off.
    hotswitch_titlebar_badge: bool = true,
        id = "hotswitch.titlebar_badge", group = HotSwitch, vis = Basic, stab = Stable,
        scope = Global;
    /// Time allowed for the second key after `Ctrl+K`.
    hotswitch_chord_timeout_ms: u16[300..=5000] = 1500,
        id = "hotswitch.chord_timeout_ms", group = HotSwitch, vis = Advanced, stab = Stable,
        scope = Global;

    // ---- Navigation & tabs (§7) --------------------------------------------------------------
    /// Open items with a single click.
    nav_single_click_open: bool = false,
        id = "nav.single_click_open", group = Nav, vis = Basic, stab = Stable, scope = Global;
    /// What Backspace does.
    nav_backspace_action: choice("back", "up") = "back",
        id = "nav.backspace_action", group = Nav, vis = Basic, stab = Stable, scope = Global;
    /// Middle-click on a folder opens a new tab.
    nav_middle_click_new_tab: bool = true,
        id = "nav.middle_click_new_tab", group = Nav, vis = Basic, stab = Stable, scope = Global;
    /// Tabs.
    tabs_enabled: bool = true,
        id = "tabs.enabled", group = Tabs, vis = Basic, stab = Stable, scope = Global;
    /// Restore tabs at startup.
    tabs_restore_session: bool = true,
        id = "tabs.restore_session", group = Tabs, vis = Basic, stab = Stable, scope = Global;
    /// A folder that is already open switches to its tab.
    tabs_dedupe: bool = false,
        id = "tabs.dedupe", group = Tabs, vis = Basic, stab = Stable, scope = Global;
    /// Split view.
    split_enabled: bool = true,
        id = "split.enabled", group = Tabs, vis = Basic, stab = Stable, scope = Global;
    /// `shell:` paths and environment variables in the address bar.
    addressbar_shell_paths: bool = true,
        id = "addressbar.shell_paths", group = Nav, vis = Advanced, stab = Stable, scope = Global;
    /// Path autocomplete.
    addressbar_autocomplete: bool = true,
        id = "addressbar.autocomplete", group = Nav, vis = Basic, stab = Stable, scope = Global;
    /// Tree mode in the sidebar.
    sidebar_tree: bool = false,
        id = "sidebar.tree", group = Sidebar, vis = Basic, stab = Stable, scope = Global;

    // ---- View --------------------------------------------------------------------------------
    /// Hidden files.
    view_show_hidden: bool = false,
        id = "view.show_hidden", group = View, vis = Basic, stab = Stable, scope = Location,
        hot = true;
    /// Protected operating-system files.
    view_show_system: bool = false,
        id = "view.show_system", group = View, vis = Advanced, stab = Stable, scope = Location;
    /// File extensions.
    view_show_extensions: bool = true,
        id = "view.show_extensions", group = View, vis = Basic, stab = Stable, scope = Location,
        hot = true;
    /// Folders above files.
    view_folders_first: bool = true,
        id = "view.folders_first", group = View, vis = Basic, stab = Stable, scope = Location;
    /// Natural order (file2 before file10).
    view_natural_sort: bool = true,
        id = "view.natural_sort", group = View, vis = Basic, stab = Stable, scope = Location;
    /// Selection checkboxes.
    view_checkboxes: bool = false,
        id = "view.checkboxes", group = View, vis = Basic, stab = Stable, scope = Global;
    /// Thumbnails.
    view_thumbnails: bool = true,
        id = "view.thumbnails", group = View, vis = Basic, stab = Stable, scope = Location,
        hot = true;
    /// Themed folder icons.
    view_themed_folder_icons: bool = true,
        id = "view.themed_folder_icons", group = View, vis = Basic, stab = Stable, scope = Global;
    /// Quick Look with Space.
    view_quick_look: bool = true,
        id = "view.quick_look", group = View, vis = Basic, stab = Stable, scope = Global;
    /// Compute folder sizes (heavy).
    view_folder_sizes: bool = false,
        id = "view.folder_sizes", group = View, vis = Advanced, stab = Beta, scope = Location;
    /// Relative dates ("2 hours ago").
    view_relative_dates: bool = false,
        id = "view.relative_dates", group = View, vis = Basic, stab = Stable, scope = Global;
    /// Remember the view mode per folder.
    view_remember_per_folder: bool = true,
        id = "view.remember_per_folder", group = View, vis = Basic, stab = Stable, scope = Global;
    /// Cut items are faded.
    view_cut_items_faded: bool = true,
        id = "view.cut_items_faded", group = View, vis = Basic, stab = Stable, scope = Global;
    /// SI units (KB = 1000).
    view_si_units: bool = false,
        id = "view.si_units", group = View, vis = Advanced, stab = Stable, scope = Global;

    // ---- File operations & safety ------------------------------------------------------------
    /// Delete moves to the Recycle Bin.
    ops_delete_to_recycle_bin: bool = true,
        id = "ops.delete_to_recycle_bin", group = Ops, vis = Basic, stab = Stable, scope = Location;
    /// Confirm deletion.
    ops_confirm_delete: bool = true,
        id = "ops.confirm_delete", group = Ops, vis = Basic, stab = Stable, scope = Global;
    /// Confirm permanent deletion (can only be turned off in Developer Mode).
    ops_confirm_permanent_delete: bool = true,
        id = "ops.confirm_permanent_delete", group = Ops, vis = Developer, stab = Stable,
        scope = Global;
    /// Undo/redo.
    ops_undo: bool = true,
        id = "ops.undo", group = Ops, vis = Basic, stab = Stable, scope = Global;
    /// Undo depth.
    ops_undo_depth: u16[1..=1000] = 100,
        id = "ops.undo_depth", group = Ops, vis = Advanced, stab = Stable, scope = Global;
    /// Parallel operations.
    ops_parallel_jobs: u8[1..=8] = 2,
        id = "ops.parallel_jobs", group = Ops, vis = Advanced, stab = Stable, scope = Global;
    /// Verify a hash after copying.
    ops_verify_copy: bool = false,
        id = "ops.verify_copy", group = Ops, vis = Basic, stab = Stable, scope = Global;
    /// Progress on the taskbar.
    ops_taskbar_progress: bool = true,
        id = "ops.taskbar_progress", group = Ops, vis = Basic, stab = Stable, scope = Global;
    /// Progress display.
    ops_progress_popup: choice("inline", "floating", "both") = "inline",
        id = "ops.progress_popup", group = Ops, vis = Basic, stab = Stable, scope = Global;
    /// Block every write operation.
    safety_readonly_mode: bool = false,
        id = "safety.readonly_mode", group = Safety, vis = Basic, stab = Stable, scope = Location,
        hot = true;
    /// Write operations in these paths need confirmation.
    safety_protected_paths: paths = Vec::new(),
        id = "safety.protected_paths", group = Safety, vis = Advanced, stab = Stable,
        scope = Global;
    /// Extra confirmation in `C:\Windows` and `Program Files`.
    safety_confirm_system_paths: bool = true,
        id = "safety.confirm_system_paths", group = Safety, vis = Basic, stab = Stable,
        scope = Global;

    // ---- Search ------------------------------------------------------------------------------
    /// Use Everything when it is running.
    search_everything: choice("auto", "off") = "auto",
        id = "search.everything", group = Search, vis = Basic, stab = Stable, scope = Global;
    /// The search box is recursive straight away.
    search_recursive_default: bool = false,
        id = "search.recursive_default", group = Search, vis = Basic, stab = Stable,
        scope = Global;
    /// Fuzzy matching in the palette and quick open.
    search_fuzzy: bool = true,
        id = "search.fuzzy", group = Search, vis = Basic, stab = Stable, scope = Global;
    /// Search file contents.
    search_content: bool = false,
        id = "search.content", group = Search, vis = Advanced, stab = Beta, scope = Global;

    // ---- System integration (all off by default; enabled in onboarding/Settings) -------------
    /// Open folders from other applications in Switch Explorer.
    integration_default_folder_handler: bool = false,
        id = "integration.default_folder_handler", group = Integration, vis = Basic,
        stab = Stable, scope = Global;
    /// Win+E opens Switch Explorer.
    integration_win_e: bool = false,
        id = "integration.win_e", group = Integration, vis = Basic, stab = Stable, scope = Global;
    /// Redirect Explorer windows that still open.
    integration_explorer_redirect: bool = false,
        id = "integration.explorer_redirect", group = Integration, vis = Developer,
        stab = Experimental, scope = Global;
    /// "Open in Switch Explorer" in Explorer's context menu.
    integration_explorer_menu_entry: bool = false,
        id = "integration.explorer_menu_entry", group = Integration, vis = Basic, stab = Stable,
        scope = Global;
    /// Send To → Google Drive entry.
    integration_sendto_drive: bool = false,
        id = "integration.sendto_drive", group = Integration, vis = Basic, stab = Stable,
        scope = Global;
    /// Keep running in the background.
    integration_resident: bool = false,
        id = "integration.resident", group = Integration, vis = Basic, stab = Stable,
        scope = Global;
    /// Global hotkey that opens a window.
    integration_global_open_hotkey: keys = String::new(),
        id = "integration.global_open_hotkey", group = Integration, vis = Advanced,
        stab = Stable, scope = Global;
    /// Panel next to Open/Save dialogs.
    integration_picker_bridge: bool = false,
        id = "integration.picker_bridge", group = Integration, vis = Basic, stab = Beta,
        scope = Global;
    /// Jump to the active folder inside an Open/Save dialog.
    integration_picker_bridge_hotkey: keys = String::from("ctrl-g"),
        id = "integration.picker_bridge_hotkey", group = Integration, vis = Advanced,
        stab = Beta, scope = Global;
    /// Replace Open/Save dialogs (Developer Mode).
    integration_picker_takeover: bool = false,
        id = "integration.picker_takeover", group = Integration, vis = Developer,
        stab = Experimental, scope = Global;
    /// Native menus follow dark mode (undocumented API).
    integration_native_menu_dark: bool = true,
        id = "integration.native_menu_dark", group = Integration, vis = Advanced, stab = Stable,
        scope = Global;

    // ---- Cloud -------------------------------------------------------------------------------
    /// The module is available (still lazy).
    cloud_enabled: bool = true,
        id = "cloud.enabled", group = Cloud, vis = Basic, stab = Stable, scope = Global;
    /// Upload/Move to Drive menu entries.
    cloud_gdrive_context_menu: bool = true,
        id = "cloud.gdrive.context_menu", group = Cloud, vis = Basic, stab = Stable,
        scope = Global, deps = ["cloud.enabled"];
    /// Drive in the sidebar after connecting.
    cloud_gdrive_browse: bool = true,
        id = "cloud.gdrive.browse", group = Cloud, vis = Basic, stab = Stable, scope = Global,
        deps = ["cloud.enabled"];
    /// Verify uploads before deleting local files (Developer Mode only to turn off).
    cloud_gdrive_verify_before_delete: bool = true,
        id = "cloud.gdrive.verify_before_delete", group = Cloud, vis = Developer, stab = Stable,
        scope = Global;
    /// Parallel transfers.
    cloud_transfer_concurrency: u8[1..=8] = 3,
        id = "cloud.transfer.concurrency", group = Cloud, vis = Advanced, stab = Stable,
        scope = Global;
    /// Chunk size in MiB (a multiple of 256 KiB).
    cloud_transfer_chunk_mib: u16[1..=256] = 8,
        id = "cloud.transfer.chunk_mib", group = Cloud, vis = Advanced, stab = Stable,
        scope = Global;
    /// Bandwidth limit (0 = unlimited).
    cloud_transfer_bandwidth_kbps: u32[0..=10_000_000] = 0,
        id = "cloud.transfer.bandwidth_kbps", group = Cloud, vis = Advanced, stab = Stable,
        scope = Global;
    /// Shut the network runtime down after this idle time.
    cloud_runtime_idle_shutdown_s: u32[10..=86400] = 300,
        id = "cloud.runtime_idle_shutdown_s", group = Cloud, vis = Advanced, stab = Stable,
        scope = Global;

    // ---- Modules, performance, developer -----------------------------------------------------
    /// Home page.
    home_enabled: bool = true,
        id = "home.enabled", group = Modules, vis = Basic, stab = Stable, scope = Global;
    /// Organizer tool (lazy).
    tools_organizer: bool = true,
        id = "tools.organizer", group = Modules, vis = Basic, stab = Stable, scope = Global;
    /// Duplicate Finder tool (lazy).
    tools_duplicates: bool = true,
        id = "tools.duplicates", group = Modules, vis = Basic, stab = Stable, scope = Global;
    /// Bulk Rename tool (lazy).
    tools_bulk_rename: bool = true,
        id = "tools.bulk_rename", group = Modules, vis = Basic, stab = Stable, scope = Global;
    /// Space Analyzer tool (lazy).
    tools_space_analyzer: bool = true,
        id = "tools.space_analyzer", group = Modules, vis = Basic, stab = Stable, scope = Global;
    /// Folder sync.
    tools_sync: bool = false,
        id = "tools.sync", group = Modules, vis = Developer, stab = Experimental, scope = Global;
    /// File tags.
    tags_enabled: bool = false,
        id = "tags.enabled", group = Modules, vis = Advanced, stab = Beta, scope = Global;
    /// System preview handlers (Office/PDF).
    module_preview_handlers: bool = true,
        id = "module.preview_handlers", group = Modules, vis = Basic, stab = Stable,
        scope = Global;
    /// Thumbnail cache budget.
    perf_thumbnail_cache_mb: u16[8..=2048] = 64,
        id = "perf.thumbnail_cache_mb", group = Perf, vis = Advanced, stab = Stable,
        scope = Global;
    /// Icon cache budget.
    perf_icon_cache_entries: u16[128..=16384] = 2048,
        id = "perf.icon_cache_entries", group = Perf, vis = Advanced, stab = Stable,
        scope = Global;
    /// Folder snapshots kept in memory.
    perf_dir_cache_count: u8[0..=64] = 8,
        id = "perf.dir_cache_count", group = Perf, vis = Advanced, stab = Stable, scope = Global;
    /// Shell STA workers.
    perf_shell_workers: u8[1..=8] = 2,
        id = "perf.shell_workers", group = Perf, vis = Advanced, stab = Stable, scope = Global,
        restart = true;
    /// Watch open folders for changes.
    perf_watcher: bool = true,
        id = "perf.watcher", group = Perf, vis = Basic, stab = Stable, scope = Global;
    /// Drop caches after a long minimize.
    perf_trim_on_minimize: bool = true,
        id = "perf.trim_on_minimize", group = Perf, vis = Advanced, stab = Stable, scope = Global;
    /// Solid material in battery saver.
    perf_solid_on_battery_saver: bool = true,
        id = "perf.solid_on_battery_saver", group = Perf, vis = Basic, stab = Stable,
        scope = Global;
    /// Animations.
    perf_animations: choice("system", "on", "off") = "system",
        id = "perf.animations", group = Perf, vis = Basic, stab = Stable, scope = Global;
    /// Developer Mode.
    dev_mode: bool = false,
        id = "dev.mode", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// Perf HUD.
    dev_perf_hud: bool = false,
        id = "dev.perf_hud", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// RAM per module.
    dev_memory_meter: bool = false,
        id = "dev.memory_meter", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// Token Inspector.
    dev_token_inspector: bool = false,
        id = "dev.token_inspector", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// Theme hot reload (on automatically in Developer Mode).
    dev_theme_hot_reload: bool = false,
        id = "dev.theme_hot_reload", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// Log level.
    dev_log_level: choice("error", "warn", "info", "debug", "trace") = "info",
        id = "dev.log_level", group = Dev, vis = Developer, stab = Stable, scope = Global;
    /// Warn about frames slower than this.
    dev_slow_frame_warn_ms: u16[1..=1000] = 24,
        id = "dev.slow_frame_warn_ms", group = Dev, vis = Developer, stab = Stable,
        scope = Global;
    /// Draw layout bounds.
    dev_show_layout_bounds: bool = false,
        id = "dev.show_layout_bounds", group = Dev, vis = Developer, stab = Stable,
        scope = Global;
}

/// Metadata of a switch by id.
pub fn meta(id: &str) -> Option<&'static FlagMeta> {
    FLAGS.iter().find(|m| m.id == id)
}

impl Features {
    /// The first dependency of `id` that is switched off, if any (for "(not in effect: …)" hints).
    pub fn blocked_by(&self, id: &str) -> Option<&'static str> {
        meta(id)?
            .depends_on
            .iter()
            .copied()
            .find(|dep| self.get(dep) == Some(FlagValue::Bool(false)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_and_fields_are_unique() {
        let ids: HashSet<_> = FLAGS.iter().map(|m| m.id).collect();
        let fields: HashSet<_> = FLAGS.iter().map(|m| m.field).collect();
        assert_eq!(ids.len(), FLAGS.len());
        assert_eq!(fields.len(), FLAGS.len());
    }

    #[test]
    fn field_names_follow_ids() {
        for m in FLAGS {
            assert_eq!(
                m.field,
                m.id.replace('.', "_"),
                "field name drifted from id"
            );
        }
    }

    #[test]
    fn defaults_satisfy_their_kind() {
        let features = Features::default();
        for m in FLAGS {
            let value = features.get(m.id).expect("every flag is readable");
            assert!(
                m.kind.check(m.id, value).is_ok(),
                "{} default out of range",
                m.id
            );
        }
    }

    #[test]
    fn hotswitchable_only_bool_or_enum() {
        for m in FLAGS.iter().filter(|m| m.hotswitchable) {
            assert!(
                matches!(m.kind, FlagKind::Bool | FlagKind::Enum(_)),
                "{} is hotswitchable but not bool/enum",
                m.id
            );
        }
    }

    #[test]
    fn dependencies_exist_and_are_bool() {
        for m in FLAGS {
            for dep in m.depends_on {
                assert_eq!(
                    meta(dep).map(|d| d.kind),
                    Some(FlagKind::Bool),
                    "{} -> {dep}",
                    m.id
                );
            }
        }
    }

    #[test]
    fn set_validates() {
        let mut f = Features::default();
        f.set("dnd.scroll_cooldown_ms", &FlagValue::Int(400))
            .unwrap();
        assert_eq!(f.dnd_scroll_cooldown_ms, 400);
        assert!(
            f.set("dnd.scroll_cooldown_ms", &FlagValue::Int(9000))
                .is_err()
        );
        f.set("dnd.default_action", &FlagValue::Text("ask".into()))
            .unwrap();
        assert_eq!(f.dnd_default_action, "ask");
        assert!(
            f.set("dnd.default_action", &FlagValue::Text("teleport".into()))
                .is_err()
        );
        assert!(f.set("no.such.flag", &FlagValue::Bool(true)).is_err());
    }

    #[test]
    fn blocked_by_master() {
        let mut f = Features::default();
        assert_eq!(f.blocked_by("dnd.source.folders"), None);
        f.dnd_enabled = false;
        assert_eq!(f.blocked_by("dnd.source.folders"), Some("dnd.enabled"));
    }
}
