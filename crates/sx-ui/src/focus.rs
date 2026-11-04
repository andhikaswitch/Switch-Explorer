//! Keyboard focus outside the file list (DESIGN.md §11, FEATURES.md "Pindah fokus area"): the
//! chrome's tab stops, `F6`/`Shift+F6` between areas, `Tab`/`Shift+Tab` between stops (inside
//! an open dialog only between its buttons), and the focus ring.
//!
//! The mouse never moves focus here: a click on a control keeps focus where it was (normally
//! the list, so its shortcuts keep working). Only the keyboard focuses a stop, so the ring stays
//! visible for as long as the stop has focus, mouse moves included. `Enter` or `Space` on a
//! focused stop clicks it (GPUI). When a focused element disappears (a closed tab, a button
//! that turned disabled), focus goes back to its area or to the list.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, ElementId, FocusHandle, InteractiveElement as _, ParentElement as _, Pixels,
    Stateful, Styled as _, Window, div,
};
use sx_theme::Token;

use crate::actions;
use crate::ops::Dialog;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Focus handles of the chrome's tab stops, kept across frames by element id (render only has
/// `&self`). An entry not used in a frame is dropped after it.
#[derive(Default)]
pub(crate) struct FocusStops {
    handles: RefCell<HashMap<ElementId, (FocusHandle, u64)>>,
    frame: Cell<u64>,
}

impl std::fmt::Debug for FocusStops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FocusStops")
            .field(&self.handles.borrow().len())
            .finish()
    }
}

impl FocusStops {
    /// The stop's handle: a tab stop (handles default to not being one).
    pub fn get(&self, id: &ElementId, cx: &App) -> FocusHandle {
        let frame = self.frame.get();
        let mut handles = self.handles.borrow_mut();
        let entry = handles
            .entry(id.clone())
            .or_insert_with(|| (cx.focus_handle().tab_stop(true), frame));
        entry.1 = frame;
        entry.0.clone()
    }

    /// The handle of a stop drawn last frame, if there was one.
    fn existing(&self, id: &str) -> Option<FocusHandle> {
        self.handles
            .borrow()
            .get(&ElementId::Name(id.to_string().into()))
            .map(|(handle, _)| handle.clone())
    }

    /// Called at the start of every frame: forgets stops the previous frame did not draw.
    pub fn begin_frame(&self) {
        let frame = self.frame.get();
        self.handles
            .borrow_mut()
            .retain(|_, (_, used)| *used == frame);
        self.frame.set(frame + 1);
    }
}

/// The focus ring: a band of `focus.ring` and `focus.ring_width` just outside the element, as
/// a child drawn over its edge while it has focus (DESIGN.md §2). Outside, it contrasts with the
/// surface the control sits on (THEMING §11 pairs), never with the control's own fill, so filled
/// buttons, the hover gradient and the Tide Slash cannot hide it; it takes no layout space. (A
/// spread shadow would not do: GPUI paints shadows as a filled shape under the element, which
/// shows through a transparent control.)
pub(crate) fn ring(radius: Pixels, cx: &App) -> Div {
    let t = cx.tokens();
    let width = t.px(Token::FocusRingWidth);
    div()
        .absolute()
        .top(-width)
        .left(-width)
        .right(-width)
        .bottom(-width)
        .rounded(radius + width)
        .border(width)
        .border_color(t.color(Token::FocusRing))
}

pub(crate) trait TabStop: Sized {
    /// Makes this element a keyboard tab stop. `ring` is the element's corner radius while it
    /// has focus (see [`Workspace::ring_for`]), `None` otherwise.
    fn tab_stop_ring(self, handle: &FocusHandle, ring: Option<Pixels>, cx: &App) -> Self;
}

impl TabStop for Stateful<Div> {
    fn tab_stop_ring(self, handle: &FocusHandle, ring_radius: Option<Pixels>, cx: &App) -> Self {
        self.track_focus(handle)
            // GPUI focuses a focusable element on any mouse button; a click here keeps focus
            // where it was. The click itself and the pressed style still happen.
            .on_any_mouse_down(|_, window, _| window.prevent_default())
            .when_some(ring_radius, |el, radius| el.child(ring(radius, cx)))
    }
}

/// An area `F6` moves between: in the tab order but not a stop itself, so focusing it and then
/// the next stop lands on its first control. A click on it does not take focus.
pub(crate) fn area(el: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    el.track_focus(handle)
        .on_any_mouse_down(|_, window, _| window.prevent_default())
}

impl Workspace {
    pub(crate) fn focus_stop(&self, id: &ElementId, cx: &App) -> FocusHandle {
        self.focus_stops.get(id, cx)
    }

    /// The ring of a stop with this corner radius, if it has focus. Only the keyboard focuses a
    /// stop, so a focused stop always shows its ring.
    pub(crate) fn ring_for(&self, handle: &FocusHandle, radius: Pixels) -> Option<Pixels> {
        (self.focused_now.as_ref() == Some(handle)).then_some(radius)
    }

    /// The areas `F6` cycles through, in screen order.
    fn focus_areas(&self) -> Vec<FocusHandle> {
        let toast = self.ops.toast.as_ref().is_some_and(|toast| toast.undo);
        [
            Some(self.command_area.clone()),
            self.sidebar_visible.then(|| self.sidebar_area.clone()),
            Some(self.list_focus.clone()),
            toast.then(|| self.toast_area.clone()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Focuses an area: the list itself, or the first control of any other area.
    fn enter_area(&self, area: &FocusHandle, window: &mut Window, cx: &mut App) {
        window.focus(area, cx);
        if area != &self.list_focus {
            window.focus_next(cx);
        }
    }

    /// `F6` / `Shift+F6`: the command bar, the sidebar (when shown), the list and the toast
    /// (when it has an action), in a cycle.
    fn focus_area(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        // A dialog keeps the focus until it closes (DESIGN.md §11).
        if self.ops.dialog.is_some() {
            return;
        }
        let areas = self.focus_areas();
        let count = areas.len();
        let list = areas
            .iter()
            .position(|a| a == &self.list_focus)
            .unwrap_or(0);
        let current = areas
            .iter()
            .position(|area| area.contains_focused(window, cx));
        let next = match current {
            Some(ix) if forward => (ix + 1) % count,
            Some(ix) => (ix + count - 1) % count,
            // Focus is outside every area (a tab in the title bar): continue from the list.
            None if forward => list,
            None => (list + count - 1) % count,
        };
        self.enter_area(&areas[next], window, cx);
        cx.notify();
    }

    /// The buttons of the open dialog, in their order on screen.
    fn dialog_stops(&self) -> Vec<FocusHandle> {
        let ids: &[&str] = match &self.ops.dialog {
            Some(Dialog::ConfirmRecycle(_) | Dialog::ConfirmDelete(_)) => {
                &["dialog-cancel", "dialog-confirm"]
            }
            Some(Dialog::Conflict(_)) => &[
                "conflict-all",
                "conflict-skip",
                "conflict-newer",
                "conflict-both",
                "conflict-replace",
            ],
            None => &[],
        };
        ids.iter()
            .filter_map(|id| self.focus_stops.existing(id))
            .collect()
    }

    /// `Tab` / `Shift+Tab`: the next or previous stop in screen order; inside an open dialog,
    /// around its buttons only.
    fn focus_stop_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.dialog.is_some() {
            let stops = self.dialog_stops();
            if stops.is_empty() {
                return;
            }
            let count = stops.len();
            let next = match stops.iter().position(|s| s.is_focused(window)) {
                Some(ix) if forward => (ix + 1) % count,
                Some(ix) => (ix + count - 1) % count,
                None if forward => 0,
                None => count - 1,
            };
            window.focus(&stops[next], cx);
        } else if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        cx.notify();
    }

    /// The focused element is gone: back to the open dialog, the area it was in, or the list.
    pub(crate) fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.dialog.is_some() {
            window.focus(&self.dialog_focus, cx);
            return;
        }
        let target = window.focus_lost_restore_target(cx);
        match target {
            Some(area) if area == self.command_area || area == self.sidebar_area => {
                self.enter_area(&area, window, cx);
            }
            _ => window.focus(&self.list_focus, cx),
        }
    }

    /// The focus actions on the workspace root.
    pub(crate) fn focus_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &actions::FocusNextArea, window, cx| {
            this.focus_area(true, window, cx);
        }))
        .on_action(
            cx.listener(|this, _: &actions::FocusPreviousArea, window, cx| {
                this.focus_area(false, window, cx);
            }),
        )
        .on_action(cx.listener(|this, _: &actions::FocusNextStop, window, cx| {
            this.focus_stop_step(true, window, cx);
        }))
        .on_action(
            cx.listener(|this, _: &actions::FocusPreviousStop, window, cx| {
                this.focus_stop_step(false, window, cx);
            }),
        )
    }
}
