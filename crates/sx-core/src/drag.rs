//! Drag Guard (DEVELOPER-MODE.md §5): whether a press may become a drag, where it may be
//! dropped, and what the drop does.
//!
//! Pure logic: time comes in as milliseconds from a monotonic clock and positions as logical
//! pixels, so every rule can be tested without a window. The UI fills a [`DragPolicy`] from the
//! switch snapshot and feeds the [`DragGuard`] its pointer events.

/// A modifier a drag may be required to start with (`dnd.require_modifier`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Alt,
    Ctrl,
    Shift,
}

/// The modifiers held at an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Modifiers {
    fn holds(self, modifier: Modifier) -> bool {
        match modifier {
            Modifier::Alt => self.alt,
            Modifier::Ctrl => self.ctrl,
            Modifier::Shift => self.shift,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// Where a drag starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOrigin {
    /// The file list (or grid).
    Content,
    /// The sidebar: pinned items and the tree.
    Sidebar,
}

/// Where a drag is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// A folder in the list, or the open folder itself.
    Folder,
    /// A sidebar item.
    Sidebar,
    /// Another application.
    External,
}

/// What `dnd.default_action` asks for when no modifier says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAction {
    /// As Explorer: move on the same volume, copy across volumes.
    System,
    Copy,
    Move,
    Ask,
}

/// Why a drag may not start: the switch that blocks it, for the hint (DESIGN.md §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// `dnd.enabled` is off.
    AllOff,
    /// `dnd.source.files` is off.
    Files,
    /// `dnd.source.folders` is off.
    Folders,
    /// `dnd.source.sidebar` is off.
    Sidebar,
}

/// What a drop does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEffect {
    /// Not allowed here: no effect, the target shows `drop_blocked`.
    None,
    Copy,
    Move,
    /// Make shortcuts.
    Link,
    /// Show the menu: copy here, move here, create shortcuts, cancel.
    Ask,
    /// A move of folders that needs the confirmation sheet first (`dnd.confirm_folder_move`).
    ConfirmMove,
}

/// The drag switches, from the settings snapshot.
// One bool per independent switch (DEVELOPER-MODE.md §5.2): a state enum would not fit.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragPolicy {
    pub enabled: bool,
    pub source_files: bool,
    pub source_folders: bool,
    pub source_sidebar: bool,
    pub external_out: bool,
    pub external_in: bool,
    pub target_sidebar: bool,
    pub target_folders: bool,
    /// `safety.readonly_mode`: nothing may be dropped into this window.
    pub readonly: bool,
    /// `dnd.threshold_px`; 0 follows the system.
    pub threshold_px: f32,
    /// The system drag threshold (`SM_CXDRAG`), used when `threshold_px` is 0.
    pub system_threshold_px: f32,
    pub hold_delay_ms: u64,
    pub scroll_cooldown_ms: u64,
    pub require_modifier: Option<Modifier>,
    pub right_drag_bypass: bool,
    pub default_action: DefaultAction,
    pub confirm_folder_move: bool,
    pub pins_locked: bool,
}

impl Default for DragPolicy {
    /// The switch defaults (DEVELOPER-MODE.md §5.2).
    fn default() -> Self {
        Self {
            enabled: true,
            source_files: true,
            source_folders: true,
            source_sidebar: true,
            external_out: true,
            external_in: true,
            target_sidebar: true,
            target_folders: true,
            readonly: false,
            threshold_px: 0.0,
            system_threshold_px: 4.0,
            hold_delay_ms: 0,
            scroll_cooldown_ms: 0,
            require_modifier: None,
            right_drag_bypass: false,
            default_action: DefaultAction::System,
            confirm_folder_move: false,
            pins_locked: false,
        }
    }
}

impl DragPolicy {
    /// Whether items may start a drag (`boleh_mulai_drag`). A selection goes whole or not at
    /// all: the first item that fails blocks it, and says why. A right-button drag with
    /// `dnd.right_drag_bypass` always may (its drop always asks).
    pub fn may_start(
        &self,
        folders: bool,
        files: bool,
        origin: DragOrigin,
        button: Button,
    ) -> Result<(), Blocked> {
        if button == Button::Right && self.right_drag_bypass {
            return Ok(());
        }
        if !self.enabled {
            return Err(Blocked::AllOff);
        }
        if folders && !self.source_folders {
            return Err(Blocked::Folders);
        }
        if files && !self.source_files {
            return Err(Blocked::Files);
        }
        if origin == DragOrigin::Sidebar && !self.source_sidebar {
            return Err(Blocked::Sidebar);
        }
        Ok(())
    }

    /// Whether a drop on `target` is allowed (`boleh_drop`, `boleh_keluar`).
    pub fn may_drop(&self, target: DropTarget) -> bool {
        match target {
            DropTarget::External => self.external_out,
            DropTarget::Sidebar => self.target_sidebar && !self.readonly,
            DropTarget::Folder => self.target_folders && !self.readonly,
        }
    }

    /// Whether drops from other applications are accepted (`boleh_masuk`).
    pub fn accepts_external(&self) -> bool {
        self.enabled && self.external_in && !self.readonly
    }

    /// Whether pinned items may be reordered (that never moves files).
    pub fn may_reorder_pins(&self) -> bool {
        self.enabled && !self.pins_locked
    }

    /// How far the pointer must move before a press becomes a drag.
    pub fn threshold(&self) -> f32 {
        if self.threshold_px > 0.0 {
            self.threshold_px
        } else {
            self.system_threshold_px
        }
    }

    /// What a drop on an allowed target inside this app does. Modifiers win (Ctrl copy, Shift
    /// move, Alt or Ctrl+Shift shortcut), then `dnd.default_action`; a right-button drag always
    /// asks. A move that carries folders asks for confirmation when `dnd.confirm_folder_move`.
    pub fn effect(
        &self,
        target: DropTarget,
        modifiers: Modifiers,
        button: Button,
        same_volume: bool,
        carries_folders: bool,
    ) -> DropEffect {
        if !self.may_drop(target) {
            return DropEffect::None;
        }
        if button == Button::Right {
            return DropEffect::Ask;
        }
        let effect = match (modifiers.ctrl, modifiers.shift, modifiers.alt) {
            (true, true, _) | (_, _, true) => DropEffect::Link,
            (true, false, false) => DropEffect::Copy,
            (false, true, false) => DropEffect::Move,
            (false, false, false) => match self.default_action {
                DefaultAction::System if same_volume => DropEffect::Move,
                DefaultAction::System | DefaultAction::Copy => DropEffect::Copy,
                DefaultAction::Move => DropEffect::Move,
                DefaultAction::Ask => DropEffect::Ask,
            },
        };
        if effect == DropEffect::Move && carries_folders && self.confirm_folder_move {
            DropEffect::ConfirmMove
        } else {
            effect
        }
    }
}

/// What a pointer event means for the press under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do yet.
    Wait,
    /// Start the drag now.
    StartDrag,
    /// The user tried to drag something a switch blocks: show the hint once.
    Hint(Blocked),
    /// The press ended without a drag: a click (select, open on double click).
    Click,
    /// The drag ended (its drop was handled by the drop target).
    Dropped,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Pressed {
        at_ms: u64,
        x: f32,
        y: f32,
        allowed: Result<(), Blocked>,
        /// `tertahan`: this press can no longer become a drag.
        held: bool,
        hinted: bool,
    },
    Dragging,
}

/// The press-to-drag state machine (DEVELOPER-MODE.md §5.4). One per window; it uses the policy
/// of the moment the button went down, so a Hot Switch never cuts a drag in progress.
#[derive(Clone, Copy, Debug)]
pub struct DragGuard {
    state: State,
    policy: Option<DragPolicy>,
}

impl Default for DragGuard {
    fn default() -> Self {
        Self {
            state: State::Idle,
            policy: None,
        }
    }
}

impl DragGuard {
    pub fn is_dragging(&self) -> bool {
        self.state == State::Dragging
    }

    /// A button went down on an item. `allowed` is [`DragPolicy::may_start`] for what would be
    /// dragged; `last_scroll_ms` is when anything in any window last scrolled.
    #[allow(clippy::too_many_arguments)]
    pub fn press(
        &mut self,
        policy: &DragPolicy,
        now_ms: u64,
        x: f32,
        y: f32,
        modifiers: Modifiers,
        last_scroll_ms: Option<u64>,
        allowed: Result<(), Blocked>,
    ) {
        let cooling =
            last_scroll_ms.is_some_and(|at| now_ms.saturating_sub(at) < policy.scroll_cooldown_ms);
        let missing_modifier = policy
            .require_modifier
            .is_some_and(|modifier| !modifiers.holds(modifier));
        self.policy = Some(*policy);
        self.state = State::Pressed {
            at_ms: now_ms,
            x,
            y,
            allowed,
            held: cooling || missing_modifier,
            hinted: false,
        };
    }

    /// The pointer moved with the button down.
    pub fn moved(&mut self, now_ms: u64, x: f32, y: f32) -> Step {
        let Some(policy) = self.policy else {
            return Step::Wait;
        };
        let State::Pressed {
            at_ms,
            x: x0,
            y: y0,
            allowed,
            held,
            hinted,
        } = self.state
        else {
            return Step::Wait;
        };
        let far = (x - x0).hypot(y - y0) >= policy.threshold();
        if !far {
            return Step::Wait;
        }
        if let Err(blocked) = allowed {
            if hinted {
                return Step::Wait;
            }
            self.state = State::Pressed {
                at_ms,
                x: x0,
                y: y0,
                allowed,
                held: true,
                hinted: true,
            };
            return Step::Hint(blocked);
        }
        if held {
            return Step::Wait;
        }
        // Not yet: the button has not been held long enough. The press stays a press, so the
        // first move after the delay starts the drag. (Cancelling it here made every quick
        // drag fail and feel heavy; a flick that is released within the delay is still a
        // click.)
        if now_ms.saturating_sub(at_ms) < policy.hold_delay_ms {
            return Step::Wait;
        }
        self.state = State::Dragging;
        Step::StartDrag
    }

    /// The button went up.
    pub fn release(&mut self) -> Step {
        let step = match self.state {
            State::Idle => Step::Wait,
            State::Pressed { .. } => Step::Click,
            State::Dragging => Step::Dropped,
        };
        self.state = State::Idle;
        self.policy = None;
        step
    }

    /// The drag or press was cancelled (Escape, window lost the capture).
    pub fn cancel(&mut self) {
        self.state = State::Idle;
        self.policy = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };

    fn guard(policy: &DragPolicy, last_scroll: Option<u64>, modifiers: Modifiers) -> DragGuard {
        let mut guard = DragGuard::default();
        let allowed = policy.may_start(false, true, DragOrigin::Content, Button::Left);
        guard.press(policy, 1_000, 10.0, 10.0, modifiers, last_scroll, allowed);
        guard
    }

    #[test]
    fn sources_follow_their_switches() {
        let on = DragPolicy::default();
        let cases = [
            // (folders, files, origin, policy change, expected)
            (false, true, DragOrigin::Content, on, Ok(())),
            (
                false,
                true,
                DragOrigin::Content,
                DragPolicy {
                    enabled: false,
                    ..on
                },
                Err(Blocked::AllOff),
            ),
            (
                true,
                false,
                DragOrigin::Content,
                DragPolicy {
                    source_folders: false,
                    ..on
                },
                Err(Blocked::Folders),
            ),
            // A mixed selection is refused whole when one kind is off.
            (
                true,
                true,
                DragOrigin::Content,
                DragPolicy {
                    source_files: false,
                    ..on
                },
                Err(Blocked::Files),
            ),
            (
                true,
                false,
                DragOrigin::Sidebar,
                DragPolicy {
                    source_sidebar: false,
                    ..on
                },
                Err(Blocked::Sidebar),
            ),
            (
                true,
                false,
                DragOrigin::Content,
                DragPolicy {
                    source_sidebar: false,
                    ..on
                },
                Ok(()),
            ),
        ];
        for (folders, files, origin, policy, expected) in cases {
            assert_eq!(
                policy.may_start(folders, files, origin, Button::Left),
                expected,
                "{folders} {files} {origin:?} {policy:?}"
            );
        }
    }

    #[test]
    fn right_drag_bypass_always_asks() {
        let policy = DragPolicy {
            enabled: false,
            right_drag_bypass: true,
            ..DragPolicy::default()
        };
        assert_eq!(
            policy.may_start(true, true, DragOrigin::Sidebar, Button::Right),
            Ok(())
        );
        assert_eq!(
            policy.may_start(true, true, DragOrigin::Sidebar, Button::Left),
            Err(Blocked::AllOff)
        );
        let open = DragPolicy {
            right_drag_bypass: true,
            ..DragPolicy::default()
        };
        assert_eq!(
            open.effect(DropTarget::Folder, NONE, Button::Right, true, false),
            DropEffect::Ask
        );
    }

    #[test]
    fn drop_targets_and_read_only() {
        let policy = DragPolicy::default();
        assert!(policy.may_drop(DropTarget::Folder) && policy.may_drop(DropTarget::Sidebar));
        let locked = DragPolicy {
            readonly: true,
            ..policy
        };
        assert!(!locked.may_drop(DropTarget::Folder) && !locked.accepts_external());
        assert!(
            locked.may_drop(DropTarget::External),
            "dragging out reads only"
        );
        let no_sidebar = DragPolicy {
            target_sidebar: false,
            ..policy
        };
        assert_eq!(
            no_sidebar.effect(DropTarget::Sidebar, NONE, Button::Left, true, false),
            DropEffect::None
        );
        assert!(
            !DragPolicy {
                pins_locked: true,
                ..policy
            }
            .may_reorder_pins()
        );
    }

    #[test]
    fn effects_follow_modifiers_then_the_default() {
        let policy = DragPolicy::default();
        let effect = |policy: &DragPolicy, modifiers: Modifiers, same: bool, folders: bool| {
            policy.effect(DropTarget::Folder, modifiers, Button::Left, same, folders)
        };
        let ctrl = Modifiers { ctrl: true, ..NONE };
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        let alt = Modifiers { alt: true, ..NONE };
        assert_eq!(effect(&policy, NONE, true, false), DropEffect::Move);
        assert_eq!(effect(&policy, NONE, false, false), DropEffect::Copy);
        assert_eq!(effect(&policy, ctrl, true, false), DropEffect::Copy);
        assert_eq!(effect(&policy, shift, false, false), DropEffect::Move);
        assert_eq!(effect(&policy, alt, true, false), DropEffect::Link);
        let ask = DragPolicy {
            default_action: DefaultAction::Ask,
            ..policy
        };
        assert_eq!(effect(&ask, NONE, true, false), DropEffect::Ask);
        let careful = DragPolicy {
            confirm_folder_move: true,
            ..policy
        };
        assert_eq!(effect(&careful, NONE, true, true), DropEffect::ConfirmMove);
        assert_eq!(effect(&careful, NONE, true, false), DropEffect::Move);
        assert_eq!(effect(&careful, ctrl, true, true), DropEffect::Copy);
    }

    #[test]
    fn a_press_becomes_a_drag_past_the_threshold() {
        let policy = DragPolicy {
            threshold_px: 14.0,
            ..DragPolicy::default()
        };
        let mut g = guard(&policy, None, NONE);
        assert_eq!(
            g.moved(1_010, 20.0, 10.0),
            Step::Wait,
            "10 px is not enough"
        );
        assert_eq!(g.moved(1_020, 25.0, 10.0), Step::StartDrag);
        assert!(g.is_dragging());
        assert_eq!(g.release(), Step::Dropped);
        // System threshold when the switch is 0.
        let mut g = guard(&DragPolicy::default(), None, NONE);
        assert_eq!(g.moved(1_001, 15.0, 10.0), Step::StartDrag);
    }

    #[test]
    fn hold_delay_and_scroll_cooldown_keep_it_a_click() {
        let hold = DragPolicy {
            hold_delay_ms: 150,
            ..DragPolicy::default()
        };
        // Moving before the delay waits; the first move after it starts the drag.
        let mut g = guard(&hold, None, NONE);
        assert_eq!(g.moved(1_050, 40.0, 10.0), Step::Wait);
        assert_eq!(g.moved(1_160, 45.0, 10.0), Step::StartDrag);
        // A flick released within the delay stays a click.
        let mut g = guard(&hold, None, NONE);
        assert_eq!(g.moved(1_050, 40.0, 10.0), Step::Wait);
        assert_eq!(g.release(), Step::Click);
        // Holding still first, then moving: a drag at once.
        let mut g = guard(&hold, None, NONE);
        assert_eq!(g.moved(1_200, 40.0, 10.0), Step::StartDrag);

        let cooldown = DragPolicy {
            scroll_cooldown_ms: 400,
            ..DragPolicy::default()
        };
        let mut g = guard(&cooldown, Some(800), NONE);
        assert_eq!(
            g.moved(1_500, 80.0, 10.0),
            Step::Wait,
            "scrolled 200 ms ago"
        );
        let mut g = guard(&cooldown, Some(500), NONE);
        assert_eq!(
            g.moved(1_010, 80.0, 10.0),
            Step::StartDrag,
            "scrolled 500 ms ago"
        );
    }

    #[test]
    fn a_required_modifier_must_be_held_at_the_press() {
        let policy = DragPolicy {
            require_modifier: Some(Modifier::Alt),
            ..DragPolicy::default()
        };
        let mut g = guard(&policy, None, NONE);
        assert_eq!(g.moved(1_010, 80.0, 10.0), Step::Wait);
        let mut g = guard(&policy, None, Modifiers { alt: true, ..NONE });
        assert_eq!(g.moved(1_010, 80.0, 10.0), Step::StartDrag);
    }

    #[test]
    fn a_blocked_drag_hints_once_and_stays_a_click() {
        let policy = DragPolicy {
            source_files: false,
            ..DragPolicy::default()
        };
        let mut g = guard(&policy, None, NONE);
        assert_eq!(g.moved(1_010, 80.0, 10.0), Step::Hint(Blocked::Files));
        assert_eq!(g.moved(1_020, 120.0, 10.0), Step::Wait);
        assert_eq!(g.release(), Step::Click);
        assert_eq!(g.release(), Step::Wait);
    }

    #[test]
    fn the_policy_of_the_press_holds_for_the_whole_drag() {
        let mut g = guard(&DragPolicy::default(), None, NONE);
        // A Hot Switch turns dragging off mid-press: this drag is not cut.
        assert_eq!(g.moved(1_010, 80.0, 10.0), Step::StartDrag);
        g.cancel();
        assert!(!g.is_dragging());
    }
}
