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
    Animation, AnimationExt as _, AnyElement, ClickEvent, Context, Div, ElementId, FontWeight,
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
    /// Fading out; gone after `motion.fast`.
    leaving: bool,
    /// A hint about this binding: a click opens the panel with it chosen.
    binding: Option<usize>,
}

/// The name of a switch (`flag.<id>`).
fn label(id: &str) -> String {
    t!(format!("flag.{id}")).into_owned()
}

/// The name of one value of an enum switch shown as on/off (`choice.<id>.<value>`).
fn choice_label(id: &str, value: &str) -> String {
    t!(format!("choice.{id}.{value}")).into_owned()
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

/// The switch holding `id` back: one it depends on that is off.
fn holder(id: &str, features: &Features) -> Option<&'static str> {
    sx_settings::meta(id)?
        .depends_on
        .iter()
        .copied()
        .find(|dep| features.get(dep) == Some(FlagValue::Bool(false)))
}

/// The switch holding every switch of `ids` back, if they all wait on one.
fn group_holder(ids: &[&str], features: &Features) -> Option<&'static str> {
    let held = ids
        .iter()
        .map(|id| holder(id, features))
        .collect::<Option<Vec<_>>>()?;
    held.first().copied()
}

/// The OSD's note for a switch turned on while `holder` is off.
fn held_note(holder: &str) -> String {
    t!("hotswitch.held", name = label(holder)).into_owned()
}

/// What the Hot Switch panel says about a switch on hold: which switch to turn on, and how.
pub(crate) fn held_hint(holder: &str) -> String {
    let name = label(holder);
    let text = if let Some(keys) = binding_keys(holder) {
        t!("hotswitch.held_hint", name = name, keys = keys)
    } else {
        t!("hotswitch.held_hint_nokeys", name = name)
    };
    text.into_owned()
}

/// The keys of the Hot Switch panel, as Windows writes them.
fn panel_keys() -> Option<String> {
    DEFAULT_BINDINGS
        .iter()
        .find(|b| matches!(b.action, HotSwitchAction::Panel))
        .map(|b| keys_text(b.keys))
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

/// What a switch change means beyond itself, as the OSD's second line says it (DESIGN.md §10:
/// "Drag file tetap aktif.").
fn impact(id: &str, on: bool, features: &Features) -> Option<String> {
    let key = match (id, on) {
        ("dnd.source.folders", false) if features.dnd_source_files => "files_still_on",
        ("dnd.source.files", false) if features.dnd_source_folders => "folders_still_on",
        ("dnd.source.sidebar", false) => "sidebar_off",
        ("dnd.enabled", false) => "dnd_off",
        ("dnd.enabled", true) => "dnd_on",
        ("view.show_hidden", true) => "hidden_on",
        ("view.show_hidden", false) => "hidden_off",
        ("view.show_extensions", true) => "extensions_on",
        ("view.show_extensions", false) => "extensions_off",
        ("safety.readonly_mode", true) => "readonly_on",
        ("safety.readonly_mode", false) => "readonly_off",
        ("sidebar.pins_locked", true) => "pins_locked_on",
        ("sidebar.pins_locked", false) => "pins_locked_off",
        ("dnd.external_out", false) => "external_off",
        ("home.show_recent", true) => "recent_on",
        ("home.show_recent", false) => "recent_off",
        _ => return None,
    };
    Some(t!(format!("hotswitch.impact.{key}")).into_owned())
}

/// The OSD's second line: what else it means, then how to switch back.
fn detail(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the OSD says about a binding that ran: (title, detail, on).
fn describe(
    binding: &HotSwitchBinding,
    changes: &[Change],
    features: &Features,
    preset_active: bool,
) -> (String, String, bool) {
    // A binding without keys says nothing about them.
    let again = (!binding.keys.is_empty())
        .then(|| t!("hotswitch.again", keys = keys_text(binding.keys)).into_owned());
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
            let on = is_on(&value);
            // Turned on while a switch it needs is off: it waits. Turned off, there is
            // nothing to add (and "still on" notes would be wrong with dragging off).
            let note = match holder(id, features) {
                Some(by) => on.then(|| held_note(by)),
                None => impact(id, on, features),
            };
            (title.into_owned(), detail(&[note, again.clone()]), on)
        }
        HotSwitchAction::Group(ids) => {
            let on = changes.first().is_some_and(|c| is_on(&c.value));
            let key = if on {
                "hotswitch.group_on"
            } else {
                "hotswitch.group_off"
            };
            let title = t!(key, name = group_label(ids), count = ids.len());
            // As for one switch: on hold says so when turned on, and nothing when turned off.
            let note = match group_holder(ids, features) {
                Some(by) => on.then(|| held_note(by)),
                None => ids.first().and_then(|id| impact(id, on, features)),
            };
            (title.into_owned(), detail(&[note, again.clone()]), on)
        }
        HotSwitchAction::Preset(id) => {
            let name = t!(format!("preset.{id}")).into_owned();
            let moved = changes.iter().filter(|c| c.previous != c.value).count();
            let (title, note) = if preset_active {
                (t!("hotswitch.preset_on", name = name, count = moved), None)
            } else {
                (
                    t!("hotswitch.preset_off", name = name),
                    Some(t!("hotswitch.restored").into_owned()),
                )
            };
            (
                title.into_owned(),
                detail(&[note, again.clone()]),
                preset_active,
            )
        }
        HotSwitchAction::Choice(id, value) => {
            let now = changes.first().map(|c| c.value.clone());
            let on = now == Some(FlagValue::Text(value.to_string()));
            let name = choice_label(id, value);
            let title = if on {
                t!("hotswitch.on", name = name)
            } else {
                t!("hotswitch.off", name = name)
            };
            // What it does now, in words: the value it went back to is never left unsaid.
            let effect = match &now {
                Some(FlagValue::Text(current)) => {
                    let key = format!("choice_effect.{id}.{current}");
                    let text = t!(&key).into_owned();
                    (text != key).then_some(text)
                }
                _ => None,
            };
            // Without keys it is switched from the panel or the palette, where the user already
            // is: line 2 says only what the switch does now (it must fit, DESIGN.md §9.1).
            (title.into_owned(), detail(&[effect, again]), on)
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

/// The hotkey of the binding that toggles `id`, for tooltips; none when it has no keys.
pub(crate) fn binding_keys(id: &str) -> Option<String> {
    DEFAULT_BINDINGS
        .iter()
        .filter(|b| !b.keys.is_empty())
        .find_map(|b| match b.action {
            HotSwitchAction::Toggle(target) if target == id => Some(keys_text(b.keys)),
            HotSwitchAction::Group(ids) if ids.contains(&id) => Some(keys_text(b.keys)),
            _ => None,
        })
}

/// The binding that toggles `id`, by index in `DEFAULT_BINDINGS`.
pub(crate) fn binding_of(id: &str) -> Option<usize> {
    DEFAULT_BINDINGS.iter().position(|b| match b.action {
        HotSwitchAction::Toggle(target) => target == id,
        HotSwitchAction::Group(ids) => ids.contains(&id),
        _ => false,
    })
}

impl Workspace {
    /// A drag or a drop a switch refuses (DESIGN.md §9.3): the OSD names the switch and how to
    /// change it; a click on it opens the Hot Switch panel with that switch chosen. It shows
    /// whatever `hotswitch.osd` says: `dnd.blocked_hint` alone decides.
    pub(crate) fn show_blocked_hint(&mut self, flag: &str, mixed: bool, cx: &mut Context<Self>) {
        // Read-only mode blocks by being on; the drag switches block by being off.
        let on_blocks = matches!(flag, "safety.readonly_mode" | "sidebar.pins_locked");
        let name = label(flag);
        let title = if on_blocks {
            t!("hotswitch.drag_blocked_on", name = name)
        } else {
            t!("hotswitch.drag_blocked", name = name)
        }
        .into_owned();
        // Keys only when they work (`hotswitch.enabled`).
        let keys = binding_keys(flag).filter(|_| self.features.hotswitch_enabled);
        let how = match (keys, on_blocks) {
            (Some(keys), false) => t!("hotswitch.drag_blocked_keys", keys = keys),
            (Some(keys), true) => t!("hotswitch.drag_blocked_keys_off", keys = keys),
            (None, _) => t!("hotswitch.drag_blocked_panel"),
        };
        let detail = if mixed {
            format!("{} {how}", t!("hotswitch.drag_blocked_mixed"))
        } else {
            how.into_owned()
        };
        self.present_osd(title, detail, false, binding_of(flag), cx);
    }

    /// A press held back by `dnd.require_modifier`: which key starts a drag.
    pub(crate) fn show_key_hint(&mut self, key: sx_core::drag::Modifier, cx: &mut Context<Self>) {
        let key = match key {
            sx_core::drag::Modifier::Alt => "Alt",
            sx_core::drag::Modifier::Ctrl => "Ctrl",
            sx_core::drag::Modifier::Shift => "Shift",
        };
        let title = t!("hotswitch.drag_needs_key", key = key).into_owned();
        let detail = t!("hotswitch.drag_needs_key_why").into_owned();
        self.present_osd(title, detail, false, None, cx);
    }

    /// The switch holding a binding back: one it depends on that is off.
    pub(crate) fn binding_holder(&self, index: usize) -> Option<&'static str> {
        match DEFAULT_BINDINGS.get(index).map(|b| b.action) {
            Some(HotSwitchAction::Toggle(id) | HotSwitchAction::Choice(id, _)) => {
                holder(id, &self.features)
            }
            Some(HotSwitchAction::Group(ids)) => group_holder(ids, &self.features),
            _ => None,
        }
    }

    /// The name of a Hot Switch binding, as a row of the palette or the panel says it.
    pub(crate) fn binding_name(index: usize) -> Option<String> {
        Some(match DEFAULT_BINDINGS.get(index)?.action {
            HotSwitchAction::Toggle(id) => label(id),
            HotSwitchAction::Group(ids) => group_label(ids),
            HotSwitchAction::Preset(id) => t!(format!("preset_title.{id}")).into_owned(),
            HotSwitchAction::Choice(id, value) => choice_label(id, value),
            HotSwitchAction::Panel => return None,
        })
    }

    /// Whether a Hot Switch binding is on now: cheap enough for every row of every frame.
    pub(crate) fn binding_on(&self, index: usize) -> Option<bool> {
        let on = |id: &str| self.features.get(id).is_some_and(|value| is_on(&value));
        Some(match DEFAULT_BINDINGS.get(index)?.action {
            HotSwitchAction::Toggle(id) => on(id),
            HotSwitchAction::Group(ids) => ids.iter().any(|id| on(id)),
            HotSwitchAction::Preset(id) => self.settings.preset_active(id),
            HotSwitchAction::Choice(id, value) => {
                self.features.get(id) == Some(FlagValue::Text(value.to_string()))
            }
            HotSwitchAction::Panel => return None,
        })
    }

    /// Runs the Hot Switch binding at `index` of `DEFAULT_BINDINGS` from its key, unless the
    /// Hot Switch keys are off (`hotswitch.enabled`).
    pub(crate) fn run_hot_switch(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.features.hotswitch_enabled {
            self.run_switch_binding(index, cx);
        }
    }

    /// Runs a Hot Switch binding and shows its OSD (also from the command palette, which the
    /// Hot Switch keys being off does not stop).
    pub(crate) fn run_switch_binding(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(binding) = DEFAULT_BINDINGS.get(index) else {
            return;
        };
        match self.settings.run_hotswitch(binding.action) {
            Ok(changes) => {
                self.apply_switch_changes(&changes, cx);
                let preset_active = match binding.action {
                    HotSwitchAction::Preset(id) => self.settings.preset_active(id),
                    _ => false,
                };
                let (title, detail, on) =
                    describe(binding, &changes, &self.features, preset_active);
                // A click on it opens the panel with this switch chosen.
                if self.features.hotswitch_osd != "off" {
                    self.present_osd(title, detail, on, Some(index), cx);
                }
            }
            Err(SettingsError::NeedsDeveloperMode(id)) => {
                let title = t!("hotswitch.needs_dev", name = label(id)).into_owned();
                let detail = t!("hotswitch.needs_dev_hint").into_owned();
                self.show_osd(title, detail, false, cx);
            }
            Err(err) => {
                tracing::warn!(error = %err, keys = binding.keys, "hot switch failed");
                let detail = if binding.keys.is_empty() {
                    t!("hotswitch.drag_blocked_panel").into_owned()
                } else {
                    t!("hotswitch.again", keys = keys_text(binding.keys)).into_owned()
                };
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
        if touches(&["home.show_recent"]) {
            // Off: the list and its memory go. On: it is read again when Home shows.
            self.refresh_home(cx);
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
        self.present_osd(title, detail, on, None, cx);
    }

    fn present_osd(
        &mut self,
        title: String,
        detail: String,
        on: bool,
        binding: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.next_osd += 1;
        let id = self.next_osd;
        let appearance = match &self.osd {
            Some(shown) if !shown.leaving => shown.appearance,
            _ => id,
        };
        let hovered = self
            .osd
            .as_ref()
            .is_some_and(|shown| shown.hovered && !shown.leaving);
        self.osd = Some(Osd {
            id,
            appearance,
            title: title.into(),
            detail: detail.into(),
            on,
            hovered,
            leaving: false,
            binding,
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
                if let Some(osd) = this.osd.as_mut()
                    && osd.id == id
                    && !osd.hovered
                {
                    osd.leaving = true;
                    this.remove_osd_later(id, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Removes a fading OSD once its fade is over (unless a switch brought it back).
    fn remove_osd_later(&mut self, id: u64, cx: &mut Context<Self>) {
        let fade = Duration::from_millis(cx.tokens().number(Token::MotionFast).max(0.0) as u64);
        let timer = cx.background_executor().timer(fade);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .osd
                    .as_ref()
                    .is_some_and(|osd| osd.id == id && osd.leaving)
                {
                    this.osd = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn hover_osd(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(osd) = self.osd.as_mut().filter(|osd| !osd.leaving) else {
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

    /// The OSD's motion: in, a fade and a rise by `space.sm` in `motion.base`; out, a fade in
    /// `motion.fast`. With reduce motion (`perf.animations = off`) or a zero duration, none.
    fn animate_osd(&self, card: Stateful<Div>, osd: &Osd, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        let motion = self.features.perf_animations != "off";
        let (ms, rise) = if osd.leaving {
            (t.number(Token::MotionFast), 0.0)
        } else {
            (t.number(Token::MotionBase), f32::from(t.px(Token::SpaceSm)))
        };
        let duration = Duration::from_millis(ms.max(0.0) as u64);
        if !motion || duration.is_zero() {
            card.when(osd.leaving, |el| el.opacity(0.0))
                .into_any_element()
        } else if osd.leaving {
            card.with_animation(
                ElementId::NamedInteger("osd-out".into(), osd.id),
                Animation::new(duration),
                |el, delta| el.opacity(1.0 - delta),
            )
            .into_any_element()
        } else {
            card.with_animation(
                ElementId::NamedInteger("osd-in".into(), osd.appearance),
                Animation::new(duration).with_easing(gpui_kit::ease_out_quint()),
                move |el, delta| el.opacity(delta).top(gpui_kit::px(rise * (1.0 - delta))),
            )
            .into_any_element()
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
        let card = div()
            .id(ElementId::NamedInteger("osd".into(), osd.appearance))
            // Pointer events stop here: the rows behind it are not hovered or clicked.
            .occlude()
            .role(gpui_kit::Role::Status)
            .aria_label(osd.title.clone())
            .aria_description(osd.detail.clone())
            .relative()
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
            // A click opens the Hot Switch panel (DESIGN.md §9.1), with the switch of a hint
            // chosen so Enter changes it.
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                let binding = this.osd.as_ref().and_then(|osd| osd.binding);
                this.open_switch_panel(binding, window, cx);
            }))
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
            );
        let card = self.animate_osd(card, osd, cx);
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
        let (hover, pressed) = (t.color(Token::HoverBg), t.color(Token::PressedBg));
        Some(
            div()
                .id("drag-chip")
                .role(gpui_kit::Role::Button)
                .aria_label(SharedString::from(t!(key)))
                .when_some(panel_keys(), |el, keys| {
                    el.aria_description(SharedString::from(t!("hotswitch.chip_hint", keys = keys)))
                })
                .flex()
                .items_center()
                .gap(t.px(Token::SpaceXs))
                // 24 px tall on the 32 px bar: a target, not just its glyphs (DESIGN.md §14).
                .px(t.px(Token::SpaceXs))
                .py(t.px(Token::SpaceXs))
                .rounded(t.px(Token::RadiusSm))
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                // The Hot Switch panel, as `Ctrl+K, K` (FEATURES.md F-KEY-04).
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_palette(crate::palette::PaletteMode::Switches, window, cx);
                }))
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
        assert_eq!(holder("dnd.source.files", &features), Some("dnd.enabled"));
        assert_eq!(holder("view.show_hidden", &features), None);
    }

    #[test]
    fn every_bound_switch_has_a_label() {
        for binding in DEFAULT_BINDINGS {
            let ids: Vec<&str> = match binding.action {
                HotSwitchAction::Toggle(id) => vec![id],
                HotSwitchAction::Group(ids) => ids.to_vec(),
                HotSwitchAction::Preset(_) | HotSwitchAction::Panel => vec![],
                HotSwitchAction::Choice(id, value) => {
                    let key = format!("choice.{id}.{value}");
                    for locale in ["en", "id"] {
                        let text = rust_i18n::t!(&key, locale = locale);
                        assert_ne!(text, key, "{key} missing in {locale}.yml");
                    }
                    vec![]
                }
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
