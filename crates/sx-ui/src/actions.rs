//! GPUI actions for the commands implemented so far, bound from the shared
//! [`CommandRegistry`](sx_core::CommandRegistry) so the keymap, palette and menus agree
//! (ARCHITECTURE.md §7). List movement keys are not commands; they are bound here directly.

use std::rc::Rc;

use gpui_kit::{Action, App, KeyBinding, KeyBindingContextPredicate, actions};
use sx_core::{CommandContext, CommandRegistry};

actions!(
    sx,
    [
        NavBack,
        NavForward,
        NavUp,
        NavRefresh,
        FocusAddress,
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
        _ => return None,
    })
}

/// Shortcuts never fire while typing in a text input (ADR-014).
fn predicate(context: CommandContext) -> Option<Rc<KeyBindingContextPredicate>> {
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
                predicate(spec.context),
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
    ]);
    cx.bind_keys(bindings);
}
