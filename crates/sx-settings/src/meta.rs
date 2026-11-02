//! Flag metadata (DEVELOPER-MODE.md §3). Settings pages, the flag page, the Hot Switch panel and the
//! keymap editor are generated from this table.

use crate::value::FlagKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Dnd,
    Sidebar,
    HotSwitch,
    Nav,
    Tabs,
    View,
    Ops,
    Safety,
    Search,
    Integration,
    Cloud,
    Modules,
    Perf,
    Dev,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visibility {
    Basic,
    Advanced,
    /// Only listed in Developer Mode.
    Developer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stability {
    Stable,
    Beta,
    /// Needs Developer Mode to enable.
    Experimental,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Global,
    /// May be overridden by Location Rules.
    Location,
}

/// Static description of one switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlagMeta {
    pub id: &'static str,
    /// Rust field name in `Features`.
    pub field: &'static str,
    pub kind: FlagKind,
    pub group: Group,
    pub visibility: Visibility,
    pub stability: Stability,
    pub scope: Scope,
    pub hotswitchable: bool,
    pub restart: bool,
    pub depends_on: &'static [&'static str],
    /// English doc comment, for tooling; user-facing text comes from i18n (`flag.<id>`).
    pub doc: &'static str,
}
