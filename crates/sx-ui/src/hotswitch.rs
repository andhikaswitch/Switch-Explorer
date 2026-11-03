//! Hot Switch (DEVELOPER-MODE.md §6, DESIGN.md §9): the default bindings flip switches at once
//! and always show what changed: the OSD, the drag status chip and the title bar badge
//! (CLAUDE.md rule 4: no switch changes silently).
//!
//! Changes persist when `hotswitch.persist` is on; `settings.toml` is written on its own
//! thread, in order, never on the UI thread.

use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, Context, Div, ElementId, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div,
};
use rust_i18n::t;
use sx_settings::{
    Change, DEFAULT_BINDINGS, Features, FlagValue, HotSwitchAction, HotSwitchBinding, PendingWrite,
    SettingsError,
};
use sx_theme::Token;

use crate::actions;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Switches whose change reloads the open folders (what is listed or how it is sorted).
const RELOAD: &[&str] = &[
    "view.show_hidden",
    "view.show_system",
    "view.folders_first",
    "view.natural_sort",
];
/// Switches whose change only redraws the rows (how names are shown).
const REDRAW: &[&str] = &["view.show_extensions"];
/// The drag switches the status chip sums up, after the master `dnd.enabled`.
const DRAG_SOURCES: &[&str] = &[
    "dnd.source.files",
    "dnd.source.folders",
    "dnd.source.sidebar",
    "dnd.external_out",
    "dnd.external_in",
];

/// Writes `settings.toml` on its own thread, in the order the writes were taken.
#[derive(Debug)]
pub(crate) struct SettingsWriter(Option<mpsc::Sender<PendingWrite>>);

impl SettingsWriter {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<PendingWrite>();
        let spawned = std::thread::Builder::new()
            .name("sx-settings".into())
            .spawn(move || {
                while let Ok(write) = rx.recv() {
                    if let Err(err) = write.write() {
                        tracing::warn!(error = %err, "settings not saved");
                    }
                }
            });
        if let Err(err) = &spawned {
            tracing::warn!(error = %err, "settings writer unavailable");
        }
        Self(spawned.ok().map(|_| tx))
    }

    fn write(&self, write: PendingWrite) {
        if let Some(tx) = &self.0
            && tx.send(write).is_err()
        {
            tracing::warn!("settings writer stopped");
        }
    }
}

/// The on-screen display of the last Hot Switch.
#[derive(Debug)]
pub(crate) struct Osd {
    /// Changes with every switch: the hide timer of an older one does nothing.
    id: u64,
    /// Changes only when the OSD appears: a switch while it is shown updates it in place.
    appearance: u64,
    title: SharedString,
    detail: SharedString,
    on: bool,
    hovered: bool,
}

/// The name of a switch (`flag.<id>`).
fn label(id: &str) -> String {
    t!(format!("flag.{id}")).into_owned()
}

/// `ctrl-k d` → `Ctrl+K, D`; `ctrl-shift-d` → `Ctrl+Shift+D`.
pub(crate) fn keys_text(keys: &str) -> String {
    keys.split(' ')
        .map(|stroke| {
            stroke
                .split('-')
                .map(|part| match part {
                    "ctrl" => "Ctrl".to_string(),
                    "shift" => "Shift".to_string(),
                    "alt" => "Alt".to_string(),
                    "win" | "cmd" | "super" => "Win".to_string(),
                    key => {
                        let mut chars = key.chars();
                        chars.next().map_or_else(String::new, |first| {
                            first.to_uppercase().chain(chars).collect()
                        })
                    }
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A sub-switch that cannot take effect because a switch it depends on is off.
fn blocked(id: &str, features: &Features) -> bool {
    sx_settings::meta(id).is_some_and(|meta| {
        meta.depends_on
            .iter()
            .any(|dep| features.get(dep) == Some(FlagValue::Bool(false)))
    })
}

fn is_on(value: &FlagValue) -> bool {
    *value != FlagValue::Bool(false)
}

/// The name of a group of switches.
fn group_label(ids: &[&str]) -> String {
    if ids == ["dnd.external_out", "dnd.external_in"] {
        return label("dnd.external");
    }
    ids.iter()
        .map(|id| label(id))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What the OSD says about a binding that ran: (title, detail, on).
fn describe(
    binding: &HotSwitchBinding,
    changes: &[Change],
    features: &Features,
    preset_active: bool,
) -> (String, String, bool) {
    let again = t!("hotswitch.again", keys = keys_text(binding.keys)).into_owned();
    match binding.action {
        HotSwitchAction::Toggle(id) => {
            let value = changes
                .first()
                .map_or(FlagValue::Bool(false), |c| c.value.clone());
            let name = label(id);
            let title = match &value {
                FlagValue::Bool(true) => t!("hotswitch.on", name = name),
                FlagValue::Bool(false) => t!("hotswitch.off", name = name),
                other => t!("hotswitch.set", name = name, value = other.to_string()),
            };
            let detail = if blocked(id, features) {
                format!("{} {again}", t!("hotswitch.blocked"))
            } else {
                again
            };
            (title.into_owned(), detail, is_on(&value))
        }
        HotSwitchAction::Group(ids) => {
            let on = changes.first().is_some_and(|c| is_on(&c.value));
            let key = if on {
                "hotswitch.group_on"
            } else {
                "hotswitch.group_off"
            };
            let title = t!(key, name = group_label(ids), count = ids.len());
            (title.into_owned(), again, on)
        }
        HotSwitchAction::Preset(id) => {
            let name = t!(format!("preset.{id}")).into_owned();
            let moved = changes.iter().filter(|c| c.previous != c.value).count();
            let title = if preset_active {
                t!("hotswitch.preset_on", name = name, count = moved)
            } else {
                t!("hotswitch.preset_off", name = name)
            };
            (title.into_owned(), again, preset_active)
        }
        HotSwitchAction::Panel => (String::new(), String::new(), true),
    }
}

/// How much dragging the switches allow, for the chip and the badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragState {
    On,
    Partial,
    Off,
}

fn drag_state(features: &Features) -> DragState {
    if !features.dnd_enabled {
        return DragState::Off;
    }
    let all = DRAG_SOURCES
        .iter()
        .all(|id| features.get(id) == Some(FlagValue::Bool(true)));
    if all {
        DragState::On
    } else {
        DragState::Partial
    }
}

/// The hotkey of the binding that toggles `id`, for tooltips.
fn binding_keys(id: &str) -> Option<String> {
    DEFAULT_BINDINGS.iter().find_map(|b| match b.action {
        HotSwitchAction::Toggle(target) if target == id => Some(keys_text(b.keys)),
        HotSwitchAction::Group(ids) if ids.contains(&id) => Some(keys_text(b.keys)),
        _ => None,
    })
}

impl Workspace {
    /// Runs the Hot Switch binding at `index` of `DEFAULT_BINDINGS`.
    pub(crate) fn run_hot_switch(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(binding) = DEFAULT_BINDINGS.get(index) else {
            return;
        };
        if !self.features.hotswitch_enabled {
            return;
        }
        match self.settings.run_hotswitch(binding.action) {
            Ok(changes) => {
                self.apply_switch_changes(&changes, cx);
                let preset_active = match binding.action {
                    HotSwitchAction::Preset(id) => self.settings.preset_active(id),
                    _ => false,
                };
                let (title, detail, on) =
                    describe(binding, &changes, &self.features, preset_active);
                self.show_osd(title, detail, on, cx);
            }
            Err(SettingsError::NeedsDeveloperMode(id)) => {
                let title = t!("hotswitch.needs_dev", name = label(id)).into_owned();
                let detail = t!("hotswitch.needs_dev_hint").into_owned();
                self.show_osd(title, detail, false, cx);
            }
            Err(err) => {
                tracing::warn!(error = %err, keys = binding.keys, "hot switch failed");
                let detail = t!("hotswitch.again", keys = keys_text(binding.keys)).into_owned();
                self.show_osd(t!("hotswitch.failed").into_owned(), detail, false, cx);
            }
        }
    }

    /// The UI follows changed switches: a new snapshot, the folders reloaded or redrawn when
    /// what they show changed, and `settings.toml` written when the change persists.
    pub(crate) fn apply_switch_changes(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        self.features = self.settings.snapshot();
        if changes.iter().any(|c| c.persisted)
            && let Some(write) = self.settings.take_write()
        {
            self.settings_writer.write(write);
        }
        let touches = |ids: &[&str]| changes.iter().any(|c| ids.contains(&c.id));
        if touches(REDRAW) {
            for tab in &self.tabs {
                tab.clear_row_cache();
            }
        }
        if touches(RELOAD) {
            let ids: Vec<u64> = self.tabs.iter().map(|tab| tab.id).collect();
            for id in ids {
                self.load_tab(id, true, cx);
            }
        }
        cx.notify();
    }

    fn show_osd(&mut self, title: String, detail: String, on: bool, cx: &mut Context<Self>) {
        // The chip and the badge update anyway; `hotswitch.osd = off` only skips the OSD.
        if self.features.hotswitch_osd == "off" {
            return;
        }
        self.next_osd += 1;
        let id = self.next_osd;
        let appearance = match &self.osd {
            Some(shown) => shown.appearance,
            None => id,
        };
        let hovered = self.osd.as_ref().is_some_and(|shown| shown.hovered);
        self.osd = Some(Osd {
            id,
            appearance,
            title: title.into(),
            detail: detail.into(),
            on,
            hovered,
        });
        self.hide_osd_later(id, cx);
        cx.notify();
    }

    /// Hides the OSD after `hotswitch.osd_ms`, unless a newer switch replaced it or the pointer
    /// holds it.
    fn hide_osd_later(&mut self, id: u64, cx: &mut Context<Self>) {
        let after = Duration::from_millis(u64::from(self.features.hotswitch_osd_ms));
        let timer = cx.background_executor().timer(after);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .osd
                    .as_ref()
                    .is_some_and(|osd| osd.id == id && !osd.hovered)
                {
                    this.osd = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn hover_osd(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(osd) = self.osd.as_mut() else {
            return;
        };
        osd.hovered = hovered;
        if !hovered {
            // Let go: the full time again from now.
            self.next_osd += 1;
            let id = self.next_osd;
            osd.id = id;
            self.hide_osd_later(id, cx);
        }
    }

    /// The OSD: centred above the status bar, never focusable (DESIGN.md §9.1).
    pub(crate) fn render_osd(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let osd = self.osd.as_ref()?;
        let t = cx.tokens();
        let circle = t.px(Token::SizeControl);
        let badge = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(circle)
            .rounded(circle / 2.0)
            .map(|el| {
                if osd.on {
                    el.bg(t.paint(Token::OsdStateOn))
                        .text_color(t.color(Token::OsdIconOn))
                        .child(Icon::new(IconName::Check).size(t.px(Token::SizeIcon)))
                } else {
                    el.bg(t.color(Token::OsdStateOff))
                        .text_color(t.color(Token::OsdIconOff))
                        .child(
                            Icon::new(gpui_kit::assets::IconName::Ban).size(t.px(Token::SizeIcon)),
                        )
                }
            });
        let fade = Duration::from_millis(t.number(Token::MotionBase).max(0.0) as u64);
        let card = div()
            .id(ElementId::NamedInteger("osd".into(), osd.appearance))
            .flex()
            .items_center()
            .gap(t.px(Token::SpaceMd))
            .min_w(t.px(Token::SizeOsdMinWidth))
            .max_w(t.px(Token::SizeOsdMaxWidth))
            .h(t.px(Token::SizeOsdHeight))
            .px(t.px(Token::SpaceLg))
            .rounded(t.px(Token::RadiusXl))
            .bg(t.paint(Token::OsdBg))
            .border_1()
            .border_color(t.color(Token::OsdBorder))
            .shadow(t.shadow(Token::ShadowPopover))
            .text_color(t.color(Token::OsdFg))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover_osd(*hovered, cx)))
            .child(badge)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(osd.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(t.px(Token::FontSizeCaption))
                            .line_height(t.px(Token::FontLineCaption))
                            .text_color(t.color(Token::FgSecondary))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(osd.detail.clone()),
                    ),
            )
            // A fade only: fine with reduce motion too (DESIGN.md §9.1).
            .with_animation(
                ElementId::NamedInteger("osd-in".into(), osd.appearance),
                Animation::new(fade),
                gpui_kit::Styled::opacity,
            );
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(t.px(Token::SizeStatusbar) + t.px(Token::SizeOsdOffset))
                .flex()
                .justify_center()
                .child(card)
                .into_any_element(),
        )
    }

    /// "Drag: on / partly on / off" in the status bar (DESIGN.md §9.2), with every drag switch
    /// and its hotkey in the tooltip.
    pub(crate) fn render_drag_chip(&self, cx: &Context<Self>) -> Option<Stateful<Div>> {
        if !self.features.hotswitch_status_chip {
            return None;
        }
        let t = cx.tokens();
        let state = drag_state(&self.features);
        let (key, color) = match state {
            DragState::On => ("hotswitch.chip_on", Token::StatuschipOn),
            DragState::Partial => ("hotswitch.chip_partial", Token::StatuschipPartial),
            DragState::Off => ("hotswitch.chip_off", Token::StatuschipOff),
        };
        // The drag arrows; struck through when dragging is off.
        let icon = match state {
            DragState::Off => Icon::new(gpui_kit::assets::IconName::Ban),
            DragState::On | DragState::Partial => {
                Icon::new(IconName::Check).path(crate::assets::MOVE_ICON)
            }
        };
        let features = self.features.clone();
        Some(
            div()
                .id("drag-chip")
                .flex()
                .items_center()
                .gap(t.px(Token::SpaceXs))
                .text_color(t.color(color))
                .child(icon.size(t.px(Token::SizeIconSm)))
                .child(SharedString::from(t!(key)))
                .tooltip(move |window, cx| {
                    let line = |id: &str| {
                        let state = if features.get(id) == Some(FlagValue::Bool(true)) {
                            t!("hotswitch.tip_on")
                        } else {
                            t!("hotswitch.tip_off")
                        };
                        match binding_keys(id) {
                            Some(keys) => format!("{}: {state} ({keys})", label(id)),
                            None => format!("{}: {state}", label(id)),
                        }
                    };
                    let text = std::iter::once("dnd.enabled")
                        .chain(DRAG_SOURCES.iter().copied())
                        .map(line)
                        .collect::<Vec<_>>()
                        .join("\n");
                    Tooltip::new(text).build(window, cx)
                }),
        )
    }

    /// "Drag off" in the title bar while all dragging is off (DESIGN.md §9.2).
    pub(crate) fn render_drag_badge(&self, cx: &Context<Self>) -> Option<Div> {
        if !self.features.hotswitch_titlebar_badge || self.features.dnd_enabled {
            return None;
        }
        let t = cx.tokens();
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .px(t.px(Token::SpaceSm))
                .rounded(t.px(Token::RadiusXl))
                .border_1()
                .border_color(t.color(Token::BorderSubtle))
                .text_size(t.px(Token::FontSizeCaption))
                .line_height(t.px(Token::FontLineCaption))
                .text_color(t.color(Token::StatuschipOff))
                .child(SharedString::from(t!("hotswitch.badge"))),
        )
    }

    /// The Hot Switch action on the workspace root.
    pub(crate) fn hotswitch_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, action: &actions::HotSwitch, _, cx| {
            this.run_hot_switch(action.binding, cx);
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_like_windows_shortcuts() {
        assert_eq!(keys_text("ctrl-k d"), "Ctrl+K, D");
        assert_eq!(keys_text("ctrl-shift-d"), "Ctrl+Shift+D");
        assert_eq!(keys_text("alt-f4"), "Alt+F4");
    }

    #[test]
    fn drag_state_follows_the_master_and_the_sources() {
        let mut features = Features::default();
        assert_eq!(drag_state(&features), DragState::On);
        features.dnd_source_folders = false;
        assert_eq!(drag_state(&features), DragState::Partial);
        features.dnd_enabled = false;
        assert_eq!(drag_state(&features), DragState::Off);
        assert!(blocked("dnd.source.files", &features));
        assert!(!blocked("view.show_hidden", &features));
    }

    #[test]
    fn every_bound_switch_has_a_label() {
        for binding in DEFAULT_BINDINGS {
            let ids: Vec<&str> = match binding.action {
                HotSwitchAction::Toggle(id) => vec![id],
                HotSwitchAction::Group(ids) => ids.to_vec(),
                HotSwitchAction::Preset(_) | HotSwitchAction::Panel => vec![],
            };
            for id in ids {
                let key = format!("flag.{id}");
                assert_ne!(label(id), key, "{key} missing in en.yml");
            }
            if let HotSwitchAction::Preset(id) = binding.action {
                let key = format!("preset.{id}");
                assert_ne!(t!(key.clone()), key, "{key} missing in en.yml");
            }
        }
    }
}
