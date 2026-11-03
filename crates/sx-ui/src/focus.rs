//! Keyboard focus outside the file list (DESIGN.md §11, FEATURES.md "Pindah fokus area"): the
//! chrome's tab stops, `F6`/`Shift+F6` between areas, `Tab`/`Shift+Tab` between stops, and the
//! focus ring. A mouse click does not move focus (the list keeps it, so its shortcuts keep
//! working); the keyboard does, and `Enter` or `Space` on a focused element clicks it (GPUI).

use std::cell::RefCell;
use std::collections::HashMap;

use gpui_kit::{
    App, BoxShadow, Context, Div, ElementId, FocusHandle, InteractiveElement as _, MouseButton,
    Stateful, Styled as _, Window, point, px,
};
use sx_theme::Token;

use crate::actions;
use crate::theme::ActiveTokens as _;
use crate::workspace::Workspace;

/// Focus handles of the chrome's tab stops, kept across frames by element id (render only has
/// `&self`).
#[derive(Default)]
pub(crate) struct FocusStops(RefCell<HashMap<ElementId, FocusHandle>>);

impl std::fmt::Debug for FocusStops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FocusStops")
            .field(&self.0.borrow().len())
            .finish()
    }
}

impl FocusStops {
    pub fn get(&self, id: &ElementId, cx: &App) -> FocusHandle {
        self.0
            .borrow_mut()
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }
}

/// The focus ring: a `focus.ring` band of `focus.ring_width` just inside the element's edge. An
/// inset shadow follows the corners, takes no layout space, and no clipping parent (the
/// sidebar's scroll area, the tab strip) cuts it off.
fn ring(cx: &App) -> BoxShadow {
    let t = cx.tokens();
    BoxShadow {
        color: t.color(Token::FocusRing),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: t.px(Token::FocusRingWidth),
        inset: true,
    }
}

pub(crate) trait TabStop: Sized {
    /// Makes this element a tab stop that shows the focus ring when the keyboard focused it.
    /// `shadows` are the element's own shadows, kept under the ring.
    fn tab_stop_ring(self, handle: &FocusHandle, shadows: Vec<BoxShadow>, cx: &App) -> Self;
}

impl TabStop for Stateful<Div> {
    fn tab_stop_ring(self, handle: &FocusHandle, shadows: Vec<BoxShadow>, cx: &App) -> Self {
        let mut focused = shadows;
        focused.push(ring(cx));
        self.track_focus(handle)
            .tab_index(0)
            // GPUI moves focus to a focusable element on mouse down; a click here keeps it
            // where it was.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .focus_visible(move |s| s.shadow(focused))
    }
}

/// An area `F6` moves between: it is in the tab order but not a stop itself, so focusing it and
/// then the next stop lands on its first control.
pub(crate) fn area(el: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    el.track_focus(handle).tab_index(0).tab_stop(false)
}

impl Workspace {
    pub(crate) fn focus_stop(&self, id: &ElementId, cx: &App) -> FocusHandle {
        self.focus_stops.get(id, cx)
    }

    /// `F6` / `Shift+F6`: the command bar, the sidebar (when shown) and the list, in a cycle.
    fn focus_area(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        // A dialog keeps the focus until it closes (DESIGN.md §11).
        if self.ops.dialog.is_some() {
            return;
        }
        let areas: Vec<FocusHandle> = [
            Some(self.command_area.clone()),
            self.sidebar_visible.then(|| self.sidebar_area.clone()),
            Some(self.list_focus.clone()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let count = areas.len();
        let current = areas
            .iter()
            .position(|area| area.contains_focused(window, cx));
        let next = match current {
            Some(ix) if forward => (ix + 1) % count,
            Some(ix) => (ix + count - 1) % count,
            // Focus is somewhere else (a tab): start from the list.
            None => count - 1,
        };
        let target = &areas[next];
        if target == &self.list_focus {
            window.focus(target, cx);
        } else {
            window.focus(target, cx);
            window.focus_next(cx);
        }
        cx.notify();
    }

    /// `Tab` / `Shift+Tab`: the next or previous stop, in screen order.
    fn focus_stop_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.dialog.is_some() {
            return;
        }
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        cx.notify();
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
