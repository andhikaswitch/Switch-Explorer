/// Who changed a setting or switch. Hot Switch changes always produce OSD feedback
/// (CLAUDE.md rule 4), so the source travels with every change event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeSource {
    /// Settings UI, palette or context menu.
    User,
    /// A Hot Switch hotkey (in-app chord or global).
    HotSwitch,
    /// `--set` on the command line.
    Cli,
    /// A preset or profile being applied or reverted.
    Preset,
    /// Loading or migrating `settings.toml`.
    File,
}

/// Light/dark state reported by the OS theme watcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemAppearance {
    pub dark: bool,
}
