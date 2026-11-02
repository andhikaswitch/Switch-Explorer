//! Back/forward history of one tab (FEATURES.md F-NAV-01).

use crate::Location;

/// Entries kept on each side; older ones are dropped.
const LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    back: Vec<Location>,
    current: Location,
    forward: Vec<Location>,
}

impl History {
    pub fn new(start: Location) -> Self {
        Self {
            back: Vec::new(),
            current: start,
            forward: Vec::new(),
        }
    }

    pub fn current(&self) -> &Location {
        &self.current
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Goes to a new location; returns `false` (and changes nothing) if it is the current one.
    pub fn navigate(&mut self, to: Location) -> bool {
        if to == self.current {
            return false;
        }
        let from = std::mem::replace(&mut self.current, to);
        push_bounded(&mut self.back, from);
        self.forward.clear();
        true
    }

    pub fn back(&mut self) -> Option<&Location> {
        let to = self.back.pop()?;
        let from = std::mem::replace(&mut self.current, to);
        push_bounded(&mut self.forward, from);
        Some(&self.current)
    }

    pub fn forward(&mut self) -> Option<&Location> {
        let to = self.forward.pop()?;
        let from = std::mem::replace(&mut self.current, to);
        push_bounded(&mut self.back, from);
        Some(&self.current)
    }

    /// Replaces the current location without a history entry (a folder that was renamed or
    /// could not be opened and fell back to its parent).
    pub fn replace(&mut self, to: Location) {
        self.current = to;
    }
}

fn push_bounded(stack: &mut Vec<Location>, location: Location) {
    if stack.len() == LIMIT {
        stack.remove(0);
    }
    stack.push(location);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Location {
        Location::path(s)
    }

    #[test]
    fn back_and_forward() {
        let mut h = History::new(p(r"C:\"));
        assert!(h.navigate(p(r"C:\Users")));
        assert!(h.navigate(p(r"C:\Users\Raka")));
        assert_eq!(h.back(), Some(&p(r"C:\Users")));
        assert_eq!(h.back(), Some(&p(r"C:\")));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward(), Some(&p(r"C:\Users")));
        assert!(h.can_go_forward());
    }

    #[test]
    fn navigating_clears_forward_and_ignores_same() {
        let mut h = History::new(p(r"C:\"));
        h.navigate(p(r"C:\a"));
        h.back();
        assert!(!h.navigate(p(r"C:\")));
        assert!(h.navigate(p(r"C:\b")));
        assert!(!h.can_go_forward());
    }

    #[test]
    fn history_is_bounded() {
        let mut h = History::new(Location::Home);
        for i in 0..200 {
            h.navigate(p(&format!(r"C:\{i}")));
        }
        let mut steps = 0;
        while h.back().is_some() {
            steps += 1;
        }
        assert_eq!(steps, LIMIT);
    }
}
