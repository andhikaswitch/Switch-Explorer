use std::collections::BTreeSet;

/// Selected rows of a list, by index into the visible order, with Explorer semantics:
/// click selects one, Ctrl+click toggles, Shift+click selects the range from the anchor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    items: BTreeSet<u32>,
    anchor: Option<u32>,
    focus: Option<u32>,
    /// Bumped on every change, so views can cache what they derive from the selection.
    revision: u64,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, ix: u32) -> bool {
        self.items.contains(&ix)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn focus(&self) -> Option<u32> {
        self.focus
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.items.iter().copied()
    }

    /// Plain click: only `ix` is selected and becomes the anchor.
    pub fn select_only(&mut self, ix: u32) {
        self.revision += 1;
        self.items.clear();
        self.items.insert(ix);
        self.anchor = Some(ix);
        self.focus = Some(ix);
    }

    /// Ctrl+click / Ctrl+Space: flip `ix`, which becomes the new anchor.
    pub fn toggle(&mut self, ix: u32) {
        self.revision += 1;
        if !self.items.remove(&ix) {
            self.items.insert(ix);
        }
        self.anchor = Some(ix);
        self.focus = Some(ix);
    }

    /// Shift+click: the range from the anchor to `ix` replaces the selection; with Ctrl it is
    /// added to it. Without an anchor this behaves like a plain click.
    pub fn extend_to(&mut self, ix: u32, additive: bool) {
        self.revision += 1;
        let Some(anchor) = self.anchor else {
            self.select_only(ix);
            return;
        };
        if !additive {
            self.items.clear();
        }
        let (lo, hi) = if anchor <= ix {
            (anchor, ix)
        } else {
            (ix, anchor)
        };
        self.items.extend(lo..=hi);
        self.focus = Some(ix);
    }

    pub fn select_all(&mut self, len: u32) {
        self.revision += 1;
        self.items = (0..len).collect();
        self.anchor = (len > 0).then_some(0);
    }

    pub fn clear(&mut self) {
        self.revision += 1;
        self.items.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// Moves the keyboard focus without changing what is selected (`Ctrl+↑/↓`); the next
    /// `Shift` extension starts from here.
    pub fn move_focus(&mut self, ix: u32) {
        self.revision += 1;
        self.focus = Some(ix);
        self.anchor = Some(ix);
    }

    /// Replaces the selection with `items` (after a re-sort or a refresh), focusing `focus`.
    pub fn restore(&mut self, items: impl IntoIterator<Item = u32>, focus: Option<u32>) {
        self.revision += 1;
        self.items = items.into_iter().collect();
        self.focus = focus;
        self.anchor = focus;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(sel: &Selection) -> Vec<u32> {
        sel.iter().collect()
    }

    #[test]
    fn click_ctrl_shift() {
        let mut sel = Selection::default();
        sel.select_only(3);
        sel.toggle(7);
        assert_eq!(items(&sel), [3, 7]);
        // Shift from the latest anchor (7) replaces the selection.
        sel.extend_to(5, false);
        assert_eq!(items(&sel), [5, 6, 7]);
        // Ctrl+Shift adds the range.
        sel.toggle(0);
        sel.extend_to(1, true);
        assert_eq!(items(&sel), [0, 1, 5, 6, 7]);
        assert_eq!(sel.focus(), Some(1));
    }

    #[test]
    fn shift_without_anchor_selects_one() {
        let mut sel = Selection::default();
        sel.extend_to(4, false);
        assert_eq!(items(&sel), [4]);
    }

    #[test]
    fn select_all_and_clear() {
        let mut sel = Selection::default();
        sel.select_all(4);
        assert_eq!(sel.len(), 4);
        sel.clear();
        assert!(sel.is_empty());
        assert_eq!(sel.focus(), None);
    }
}
