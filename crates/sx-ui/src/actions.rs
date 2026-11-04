//! GPUI actions for the commands implemented so far, bound from the shared
//! [`CommandRegistry`](sx_core::CommandRegistry) so the keymap, palette and menus agree
//! (ARCHITECTURE.md §7). List movement keys are not commands; they are bound here directly.

use std::rc::Rc;

use gpui_kit::{Action, App, KeyBinding, KeyBindingContextPredicate, actions};
use sx_core::{CommandContext, CommandRegistry};
use sx_settings::{DEFAULT_BINDINGS, HotSwitchAction};

/// Runs a Hot Switch binding: its index in `sx_settings::DEFAULT_BINDINGS`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = sx, no_json)]
pub struct HotSwitch {
    pub binding: usize,
}

actions!(
    sx,
    [
        NavBack,
        NavForward,
        NavUp,
        NavRefresh,
        FocusAddress,
        SearchFilter,
        TabNew,
        TabClose,
        TabNext,
        TabPrevious,
        OpenSelected,
        OpenInNewTab,
        SelectAll,
        ToggleSidebar,
        SelectPrevious,
        SelectNext,
        SelectFirst,
        SelectLast,
        SelectPageUp,
        SelectPageDown,
        ExtendPrevious,
        ExtendNext,
        ExtendFirst,
        ExtendLast,
        MoveFocusPrevious,
        MoveFocusNext,
        ToggleFocused,
        FileDelete,
        FileDeletePermanent,
        FileRename,
        FileNewFolder,
        FileCopy,
        FileCut,
        FilePaste,
        FileCopyPath,
        EditUndo,
        EditRedo,
        TransferPanel,
        SidebarPin,
        FocusNextArea,
        FocusPreviousArea,
        FocusNextStop,
        FocusPreviousStop,
    ]
);

/// Key context of the file list (focused) and of the window (`CommandContext` names).
pub const LIST_CONTEXT: &str = "List";
pub const WORKSPACE_CONTEXT: &str = "Workspace";

/// The action of a command id, for the commands this build implements.
fn action_for(id: &str) -> Option<Box<dyn Action>> {
    Some(match id {
        "nav.back" => Box::new(NavBack),
        "nav.forward" => Box::new(NavForward),
        "nav.up" => Box::new(NavUp),
        "nav.refresh" => Box::new(NavRefresh),
        "nav.focus_address" => Box::new(FocusAddress),
        "search.filter" => Box::new(SearchFilter),
        "tab.new" => Box::new(TabNew),
        "tab.close" => Box::new(TabClose),
        "tab.next" => Box::new(TabNext),
        "tab.previous" => Box::new(TabPrevious),
        "file.open" => Box::new(OpenSelected),
        "edit.select_all" => Box::new(SelectAll),
        "edit.toggle_focused" => Box::new(ToggleFocused),
        "view.toggle_sidebar" => Box::new(ToggleSidebar),
        "file.delete" => Box::new(FileDelete),
        "file.delete_permanent" => Box::new(FileDeletePermanent),
        "file.rename" => Box::new(FileRename),
        "file.new_folder" => Box::new(FileNewFolder),
        "file.copy" => Box::new(FileCopy),
        "file.cut" => Box::new(FileCut),
        "file.paste" => Box::new(FilePaste),
        "file.copy_path" => Box::new(FileCopyPath),
        "edit.undo" => Box::new(EditUndo),
        "edit.redo" => Box::new(EditRedo),
        "app.transfer_panel" => Box::new(TransferPanel),
        "sidebar.pin" => Box::new(SidebarPin),
        "view.next_area" => Box::new(FocusNextArea),
        "view.previous_area" => Box::new(FocusPreviousArea),
        _ => return None,
    })
}

/// Commands that also work while typing: moving between areas leaves a text input.
const IN_INPUTS: &[&str] = &["view.next_area", "view.previous_area"];

/// Shortcuts never fire while typing in a text input (ADR-014), except [`IN_INPUTS`].
fn predicate(id: &str, context: CommandContext) -> Option<Rc<KeyBindingContextPredicate>> {
    if IN_INPUTS.contains(&id) {
        return KeyBindingContextPredicate::parse(WORKSPACE_CONTEXT)
            .ok()
            .map(Rc::new);
    }
    let source = match context {
        CommandContext::Workspace => "Workspace && !Input",
        CommandContext::List => "List && !Input",
        CommandContext::Input => "Input",
        CommandContext::Dialog => "Dialog",
    };
    KeyBindingContextPredicate::parse(source).ok().map(Rc::new)
}

pub fn bind(registry: &CommandRegistry, cx: &mut App) {
    let mut bindings = Vec::new();
    for spec in registry.iter() {
        let Some(action) = action_for(spec.id) else {
            continue;
        };
        for keys in spec.bindings {
            match KeyBinding::load(
                keys,
                action.boxed_clone(),
                predicate(spec.id, spec.context),
                false,
                None,
                cx,
            ) {
                Ok(binding) => bindings.push(binding),
                Err(err) => tracing::warn!(command = spec.id, keys, error = ?err, "bad binding"),
            }
        }
    }
    let list = Some("List && !Input");
    bindings.extend([
        KeyBinding::new("up", SelectPrevious, list),
        KeyBinding::new("down", SelectNext, list),
        KeyBinding::new("home", SelectFirst, list),
        KeyBinding::new("end", SelectLast, list),
        KeyBinding::new("pageup", SelectPageUp, list),
        KeyBinding::new("pagedown", SelectPageDown, list),
        KeyBinding::new("shift-up", ExtendPrevious, list),
        KeyBinding::new("shift-down", ExtendNext, list),
        KeyBinding::new("shift-home", ExtendFirst, list),
        KeyBinding::new("shift-end", ExtendLast, list),
        KeyBinding::new("ctrl-up", MoveFocusPrevious, list),
        KeyBinding::new("ctrl-down", MoveFocusNext, list),
        KeyBinding::new("ctrl-enter", OpenInNewTab, list),
        // Moving between controls is not a command; it also leaves a text input.
        KeyBinding::new("tab", FocusNextStop, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPreviousStop, Some(WORKSPACE_CONTEXT)),
    ]);
    bind_hot_switches(&mut bindings, cx);
    cx.bind_keys(bindings);
}

/// The Hot Switch bindings (DEVELOPER-MODE.md §6.2). Chords (`Ctrl+K`, then a letter) are off
/// in text inputs, where they would eat what is typed; single combinations work anywhere.
fn bind_hot_switches(bindings: &mut Vec<KeyBinding>, cx: &App) {
    for (binding, spec) in DEFAULT_BINDINGS.iter().enumerate() {
        // The Hot Switch panel comes later; its keys stay free.
        if matches!(spec.action, HotSwitchAction::Panel) {
            continue;
        }
        let context = if spec.keys.contains(' ') {
            "Workspace && !Input"
        } else {
            WORKSPACE_CONTEXT
        };
        match KeyBinding::load(
            spec.keys,
            Box::new(HotSwitch { binding }),
            KeyBindingContextPredicate::parse(context).ok().map(Rc::new),
            false,
            None,
            cx,
        ) {
            Ok(key) => bindings.push(key),
            Err(err) => tracing::warn!(keys = spec.keys, error = ?err, "bad hot switch binding"),
        }
    }
}
