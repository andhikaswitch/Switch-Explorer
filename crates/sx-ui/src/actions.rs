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
        SearchFind,
        CommandPalette,
        QuickOpen,
        NativeMenu,
        ContextMenuKey,
        FileProperties,
        HotSwitchPanel,
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
        OpenTerminal,
        OpenInCode,
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
        "search.find" => Box::new(SearchFind),
        "app.command_palette" => Box::new(CommandPalette),
        "app.quick_open" => Box::new(QuickOpen),
        "menu.more_options" => Box::new(NativeMenu),
        "menu.context" => Box::new(ContextMenuKey),
        "file.open_new_tab" => Box::new(OpenInNewTab),
        "file.properties" => Box::new(FileProperties),
        "hotswitch.panel" => Box::new(HotSwitchPanel),
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
        "file.open_terminal" => Box::new(OpenTerminal),
        "file.open_vscode" => Box::new(OpenInCode),
        "edit.undo" => Box::new(EditUndo),
        "edit.redo" => Box::new(EditRedo),
        "app.transfer_panel" => Box::new(TransferPanel),
        "sidebar.pin" => Box::new(SidebarPin),
        "view.next_area" => Box::new(FocusNextArea),
        "view.previous_area" => Box::new(FocusPreviousArea),
        _ => return None,
    })
}

/// The commands the palette offers (shown there and implemented here).
pub(crate) fn palette_commands() -> Vec<(&'static sx_core::CommandSpec, Box<dyn Action>)> {
    sx_core::command::BUILTIN
        .iter()
        .filter(|spec| spec.in_palette)
        .filter_map(|spec| action_for(spec.id).map(|action| (spec, action)))
        .collect()
}

/// Commands that also work while typing: moving between areas leaves a text input.
const IN_INPUTS: &[&str] = &["view.next_area", "view.previous_area"];

/// The palette's modes: their shortcuts also work in its field, to switch mode (DESIGN.md §8.9).
const IN_PALETTE: &[&str] = &["app.command_palette", "app.quick_open", "hotswitch.panel"];

/// The key context of the open palette.
pub(crate) const PALETTE_CONTEXT: &str = "Palette";

/// Shortcuts never fire while typing in a text input (ADR-014), except [`IN_INPUTS`].
fn predicate(id: &str, context: CommandContext) -> Option<Rc<KeyBindingContextPredicate>> {
    if IN_INPUTS.contains(&id) {
        return KeyBindingContextPredicate::parse(WORKSPACE_CONTEXT)
            .ok()
            .map(Rc::new);
    }
    let source = match context {
        CommandContext::Workspace => "Workspace && !Input && !ContextMenu",
        CommandContext::List => "List && !Input && !ContextMenu",
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
        let in_palette = IN_PALETTE
            .contains(&spec.id)
            .then(|| {
                KeyBindingContextPredicate::parse(PALETTE_CONTEXT)
                    .ok()
                    .map(Rc::new)
            })
            .flatten();
        for keys in spec.bindings {
            for context in [predicate(spec.id, spec.context), in_palette.clone()] {
                if context.is_none() {
                    continue;
                }
                match KeyBinding::load(keys, action.boxed_clone(), context, false, None, cx) {
                    Ok(binding) => bindings.push(binding),
                    Err(err) => {
                        tracing::warn!(command = spec.id, keys, error = ?err, "bad binding");
                    }
                }
            }
        }
    }
    let list = Some("List && !Input && !ContextMenu");
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
        // The panel is a command of the registry (`hotswitch.panel`), bound with the others;
        // a binding without keys lives in the panel and the palette only.
        if matches!(spec.action, HotSwitchAction::Panel) || spec.keys.is_empty() {
            continue;
        }
        let context = if spec.keys.contains(' ') {
            "Workspace && !Input && !ContextMenu"
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
