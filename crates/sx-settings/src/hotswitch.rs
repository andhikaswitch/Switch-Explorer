//! Default Hot Switch bindings (DEVELOPER-MODE.md §6.2) and presets (§5.5, §8).

/// What a Hot Switch binding does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotSwitchAction {
    /// Flip a bool, or step an enum to its next variant.
    Toggle(&'static str),
    /// Several switches move together: if any is on all go off, otherwise all go on.
    Group(&'static [&'static str]),
    /// Turn a preset on (remembering the previous values) or back off (restoring them).
    Preset(&'static str),
    /// Open the Hot Switch panel.
    Panel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotSwitchBinding {
    /// GPUI keystrokes (`ctrl-k d`).
    pub keys: &'static str,
    pub action: HotSwitchAction,
    /// Single combinations may be registered system-wide with `RegisterHotKey`.
    pub may_be_global: bool,
}

const fn bind(keys: &'static str, action: HotSwitchAction) -> HotSwitchBinding {
    HotSwitchBinding {
        keys,
        action,
        may_be_global: false,
    }
}

pub const DEFAULT_BINDINGS: &[HotSwitchBinding] = &[
    HotSwitchBinding {
        keys: "ctrl-shift-d",
        action: HotSwitchAction::Toggle("dnd.enabled"),
        may_be_global: true,
    },
    bind("ctrl-k f", HotSwitchAction::Toggle("dnd.source.files")),
    bind("ctrl-k d", HotSwitchAction::Toggle("dnd.source.folders")),
    bind("ctrl-k s", HotSwitchAction::Toggle("dnd.source.sidebar")),
    bind(
        "ctrl-k e",
        HotSwitchAction::Group(&["dnd.external_out", "dnd.external_in"]),
    ),
    bind("ctrl-k g", HotSwitchAction::Preset("anti_drag")),
    bind("ctrl-k l", HotSwitchAction::Toggle("sidebar.pins_locked")),
    bind("ctrl-k h", HotSwitchAction::Toggle("view.show_hidden")),
    bind("ctrl-k x", HotSwitchAction::Toggle("view.show_extensions")),
    bind("ctrl-k t", HotSwitchAction::Toggle("view.thumbnails")),
    bind("ctrl-k r", HotSwitchAction::Toggle("safety.readonly_mode")),
    bind("ctrl-k k", HotSwitchAction::Panel),
];

/// A named set of switch values (`--set` syntax for the values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub values: &'static [(&'static str, &'static str)],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "anti_drag",
        values: &[
            ("dnd.source.sidebar", "false"),
            ("dnd.threshold_px", "14"),
            ("dnd.hold_delay_ms", "150"),
            ("dnd.scroll_cooldown_ms", "400"),
            ("dnd.default_action", "ask"),
            ("dnd.confirm_folder_move", "true"),
            ("dnd.spring_open", "false"),
            ("dnd.right_drag_bypass", "true"),
            ("sidebar.pins_locked", "true"),
        ],
    },
    Preset {
        id: "low_memory",
        values: &[
            ("view.thumbnails", "false"),
            ("perf.thumbnail_cache_mb", "16"),
            ("perf.icon_cache_entries", "1024"),
            ("perf.dir_cache_count", "2"),
            ("perf.shell_workers", "1"),
            ("module.preview_handlers", "false"),
            ("cloud.gdrive.browse", "false"),
        ],
    },
    Preset {
        id: "power_user",
        values: &[
            ("view.show_hidden", "true"),
            ("tabs.dedupe", "true"),
            ("view.relative_dates", "true"),
            ("sidebar.tree", "true"),
        ],
    },
    Preset {
        id: "read_only",
        values: &[("safety.readonly_mode", "true"), ("dnd.enabled", "false")],
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::{Features, meta};

    #[test]
    fn bindings_target_hotswitchable_flags() {
        for binding in DEFAULT_BINDINGS {
            let ids: &[&str] = match &binding.action {
                HotSwitchAction::Toggle(id) => std::slice::from_ref(id),
                HotSwitchAction::Group(ids) => ids,
                HotSwitchAction::Preset(id) => {
                    assert!(preset(id).is_some(), "unknown preset {id}");
                    &[]
                }
                HotSwitchAction::Panel => &[],
            };
            for id in ids {
                let m = meta(id).unwrap_or_else(|| panic!("unknown flag {id}"));
                assert!(m.hotswitchable, "{id} is bound but not hotswitchable");
            }
            // Only single combinations may go global (chords cannot be registered).
            if binding.may_be_global {
                assert!(!binding.keys.contains(' '));
            }
        }
    }

    #[test]
    fn preset_values_are_valid() {
        for p in PRESETS {
            let mut features = Features::default();
            for (id, raw) in p.values {
                let kind = meta(id)
                    .unwrap_or_else(|| panic!("{}: unknown {id}", p.id))
                    .kind;
                let value = kind.parse_cli(id, raw).unwrap();
                features.set(id, &value).unwrap();
            }
        }
    }
}
