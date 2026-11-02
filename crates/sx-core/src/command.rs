//! The command registry: every user action is declared once here (stable id, i18n label key,
//! context, default bindings, where it shows up). Context menus, the palette, the toolbar, the
//! keymap editor and Hot Switch all read the same registry (ARCHITECTURE.md §7).

use std::collections::HashMap;

use crate::error::CoreError;

/// Stable dotted command id (`file.copy`). Never renamed after release; add an alias instead.
pub type CommandId = &'static str;

/// Where a binding is active. GPUI key contexts use the same names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandContext {
    /// Anywhere in a window except text input.
    Workspace,
    /// The focused file list or grid.
    List,
    /// A text input (address bar, search, rename).
    Input,
    /// A modal dialog or sheet.
    Dialog,
}

impl CommandContext {
    pub fn key_context(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::List => "List",
            Self::Input => "Input",
            Self::Dialog => "Dialog",
        }
    }
}

/// Static description of one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: CommandId,
    pub context: CommandContext,
    /// Default keystrokes in GPUI syntax (`ctrl-shift-n`, `ctrl-k d`).
    pub bindings: &'static [&'static str],
    pub in_palette: bool,
    pub in_context_menu: bool,
}

impl CommandSpec {
    /// i18n key of the label: `command.<id>`.
    pub fn label_key(&self) -> String {
        format!("command.{}", self.id)
    }
}

/// All commands of the app, in declaration order.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
    index: HashMap<CommandId, usize>,
}

impl CommandRegistry {
    /// The registry with every built-in command.
    pub fn builtin() -> Result<Self, CoreError> {
        let mut registry = Self::default();
        for spec in BUILTIN {
            registry.register(spec.clone())?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, spec: CommandSpec) -> Result<(), CoreError> {
        if self.index.contains_key(spec.id) {
            return Err(CoreError::DuplicateCommand(spec.id));
        }
        self.index.insert(spec.id, self.commands.len());
        self.commands.push(spec);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.index.get(id).map(|&ix| &self.commands[ix])
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.commands.iter()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

const fn cmd(
    id: CommandId,
    context: CommandContext,
    bindings: &'static [&'static str],
    in_palette: bool,
    in_context_menu: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        context,
        bindings,
        in_palette,
        in_context_menu,
    }
}

use CommandContext::{List, Workspace};

/// Built-in commands with the default shortcuts of FEATURES.md ("Shortcut default").
/// Hot Switch bindings live with the switches in `sx-settings`.
pub const BUILTIN: &[CommandSpec] = &[
    cmd("window.new", Workspace, &["ctrl-n"], true, false),
    cmd("tab.new", Workspace, &["ctrl-t"], true, false),
    cmd("tab.close", Workspace, &["ctrl-w"], true, false),
    cmd("tab.next", Workspace, &["ctrl-tab"], true, false),
    cmd("tab.previous", Workspace, &["ctrl-shift-tab"], true, false),
    cmd(
        "nav.back",
        Workspace,
        &["alt-left", "backspace"],
        true,
        false,
    ),
    cmd("nav.forward", Workspace, &["alt-right"], true, false),
    cmd("nav.up", Workspace, &["alt-up"], true, false),
    cmd("nav.refresh", Workspace, &["f5"], true, false),
    cmd(
        "nav.focus_address",
        Workspace,
        &["alt-d", "ctrl-l", "f4"],
        true,
        false,
    ),
    cmd("search.filter", Workspace, &["ctrl-f"], true, false),
    cmd("search.find", Workspace, &["ctrl-e", "f3"], true, false),
    cmd("file.open", List, &["enter"], false, true),
    cmd("file.rename", List, &["f2"], true, true),
    cmd("file.properties", List, &["alt-enter"], true, true),
    cmd("file.new_folder", Workspace, &["ctrl-shift-n"], true, true),
    cmd("file.delete", List, &["delete", "ctrl-d"], true, true),
    cmd(
        "file.delete_permanent",
        List,
        &["shift-delete"],
        true,
        false,
    ),
    cmd("file.copy", List, &["ctrl-c"], true, true),
    cmd("file.cut", List, &["ctrl-x"], true, true),
    cmd("file.paste", Workspace, &["ctrl-v"], true, true),
    cmd("file.copy_path", List, &["ctrl-shift-c"], true, true),
    cmd("edit.undo", Workspace, &["ctrl-z"], true, false),
    cmd("edit.redo", Workspace, &["ctrl-y"], true, false),
    cmd("edit.select_all", List, &["ctrl-a"], true, false),
    cmd("edit.toggle_focused", List, &["ctrl-space"], false, false),
    cmd("view.quick_look", List, &["space"], true, false),
    cmd("view.preview_pane", Workspace, &["alt-p"], true, false),
    cmd(
        "view.details_pane",
        Workspace,
        &["alt-shift-p"],
        true,
        false,
    ),
    cmd("view.split", Workspace, &["ctrl-\\"], true, false),
    cmd("view.next_area", Workspace, &["f6"], false, false),
    cmd("view.toggle_sidebar", Workspace, &["ctrl-b"], true, false),
    cmd("view.fullscreen", Workspace, &["f11"], true, false),
    cmd(
        "app.command_palette",
        Workspace,
        &["ctrl-shift-p", "f1"],
        false,
        false,
    ),
    cmd("app.quick_open", Workspace, &["ctrl-p"], true, false),
    cmd("app.transfer_panel", Workspace, &["ctrl-j"], true, false),
    cmd("app.settings", Workspace, &["ctrl-,"], true, false),
    cmd("hotswitch.panel", Workspace, &["ctrl-k k"], true, false),
    // No default shortcut, like Explorer; the star in the command bar and the palette.
    cmd("sidebar.pin", Workspace, &[], true, true),
    cmd("theme.reset", Workspace, &["ctrl-alt-shift-t"], true, false),
    cmd(
        "dev.token_inspector",
        Workspace,
        &["ctrl-shift-i"],
        true,
        false,
    ),
    cmd("dev.perf_hud", Workspace, &["ctrl-shift-f12"], true, false),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_unique_and_dotted() {
        // Commands Explorer has no shortcut for either; reached from the UI and the palette.
        const UNBOUND: &[&str] = &["sidebar.pin"];
        let registry = CommandRegistry::builtin().expect("built-in commands are unique");
        assert_eq!(registry.len(), BUILTIN.len());
        for spec in registry.iter() {
            assert!(spec.id.contains('.'), "{} is not a dotted id", spec.id);
            assert!(
                !spec.bindings.is_empty() || UNBOUND.contains(&spec.id),
                "{} has no default binding",
                spec.id
            );
        }
    }

    #[test]
    fn bindings_are_not_shared() {
        let mut seen = HashMap::new();
        for spec in BUILTIN {
            for binding in spec.bindings {
                if let Some(other) = seen.insert(*binding, spec.id) {
                    panic!("{binding} bound to both {other} and {}", spec.id);
                }
            }
        }
    }

    #[test]
    fn duplicate_registration_fails() {
        let mut registry = CommandRegistry::builtin().expect("built-in commands are unique");
        let err = registry.register(BUILTIN[0].clone());
        assert_eq!(err, Err(CoreError::DuplicateCommand("window.new")));
        assert_eq!(
            registry.get("file.copy").map(CommandSpec::label_key),
            Some("command.file.copy".into())
        );
    }
}
