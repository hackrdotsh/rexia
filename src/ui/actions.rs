//! The actions rexia performs itself, named as Rex for Mac names them so init.lua bindings,
//! `rex do` and `client.perform` work the same, with Linux default keys.
use serde_json::{Value, json};

pub struct ActionDef {
    pub name: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub keywords: &'static str,
    /// Default bindings: keys (one per stroke of a sequence) and fixed arguments.
    pub keys: &'static [(&'static [&'static str], Option<&'static str>)],
    /// A JSON Schema of the arguments, when the action takes any.
    pub args: Option<&'static str>,
    pub repeats: bool,
}

const DIRECTION: &str = r#"{"type":"object","properties":{"direction":{"type":"string","enum":["left","right","up","down"]}},"required":["direction"]}"#;
const SPLIT: &str = r#"{"type":"object","properties":{"direction":{"type":"string","enum":["left","right","up","down","auto"]}}}"#;
const RESIZE: &str = r#"{"type":"object","properties":{"direction":{"type":"string","enum":["left","right","up","down"]},"amount":{"type":"number"}},"required":["direction"]}"#;
const INDEX: &str = r#"{"type":"object","properties":{"index":{"type":"integer","minimum":1}},"required":["index"]}"#;
const SESSION: &str =
    r#"{"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"]}"#;
const MODE: &str =
    r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#;
const KEY: &str = r#"{"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}"#;
const HOST: &str = r#"{"type":"object","properties":{"host":{"type":"string"}}}"#;

macro_rules! action {
    ($name:literal, $title:literal, $category:literal, $keywords:literal, [$($keys:expr),* $(,)?] $(, args = $args:expr)? $(, repeats = $repeats:expr)?) => {
        ActionDef {
            name: $name,
            title: $title,
            category: $category,
            keywords: $keywords,
            keys: &[$($keys),*],
            args: { #[allow(unused_mut, unused_assignments)] let mut a = None; $(a = Some($args);)? a },
            repeats: { #[allow(unused_mut, unused_assignments)] let mut r = false; $(r = $repeats;)? r },
        }
    };
}

pub const ACTIONS: &[ActionDef] = &[
    // Window and app
    action!(
        "client.close",
        "Close",
        "Window",
        "close pane tab window focused",
        [(&["ctrl+shift+w"], None)]
    ),
    action!(
        "client.window.fullscreen.toggle",
        "Toggle Full Screen",
        "Window",
        "full screen fullscreen",
        [(&["f11"], None)]
    ),
    action!(
        "client.version.copy",
        "Copy Version Info",
        "App",
        "version build copy clipboard",
        []
    ),
    action!(
        "client.debug.copy",
        "Copy Debug Info",
        "App",
        "debug report diagnostics copy clipboard",
        []
    ),
    action!(
        "client.notice.dismiss",
        "Dismiss Notice",
        "App",
        "dismiss notice banner",
        [(&["ctrl+alt+shift+."], None)]
    ),
    action!(
        "client.config.copy_path",
        "Copy Config File Path",
        "View",
        "config init.lua path copy",
        []
    ),
    action!(
        "client.config.reload",
        "Reload Config",
        "View",
        "config init.lua reload keybindings",
        []
    ),
    action!(
        "client.palette.toggle",
        "Command Palette…",
        "App",
        "command palette search commands actions",
        [(&["ctrl+shift+p"], None)]
    ),
    action!(
        "client.settings",
        "Settings…",
        "App",
        "preferences settings options configure",
        [(&["ctrl+,"], None)]
    ),
    action!(
        "client.diagnostics",
        "Rex Diagnostics",
        "App",
        "diagnostics logs debug troubleshoot report keymap",
        []
    ),
    action!(
        "client.find.open",
        "Find…",
        "Find",
        "find search text scrollback highlight",
        [(&["ctrl+shift+f"], None)]
    ),
    action!(
        "client.find.next",
        "Find Next",
        "Find",
        "find search next match",
        [(&["f3"], None)]
    ),
    action!(
        "client.find.previous",
        "Find Previous",
        "Find",
        "find search previous match",
        [(&["shift+f3"], None)]
    ),
    action!(
        "client.mode.enter",
        "Enter Mode",
        "Keys",
        "mode key table prefix",
        [],
        args = MODE
    ),
    action!(
        "client.mode.exit",
        "Exit Mode",
        "Keys",
        "mode exit leave",
        []
    ),
    action!(
        "client.mode.exit_all",
        "Exit All Modes",
        "Keys",
        "mode exit leave all",
        []
    ),
    // Sessions and hosts
    action!(
        "session.new",
        "New Session",
        "Session",
        "create session add",
        [(&["ctrl+alt+shift+n"], None)]
    ),
    action!(
        "session.switch",
        "Change Session…",
        "Session",
        "session switch jump go",
        [(&["ctrl+shift+k"], None)]
    ),
    action!(
        "session.rename",
        "Rename Session",
        "Session",
        "rename session name title",
        []
    ),
    action!(
        "session.close",
        "Close Session",
        "Session",
        "close kill destroy delete session",
        []
    ),
    action!(
        "session.select",
        "Go to Session",
        "Session",
        "session select show",
        [],
        args = SESSION
    ),
    action!(
        "session.next",
        "Next Session",
        "Session",
        "cycle session forward",
        [(&["ctrl+alt+shift+down"], None)]
    ),
    action!(
        "session.previous",
        "Previous Session",
        "Session",
        "cycle session back",
        [(&["ctrl+alt+shift+up"], None)]
    ),
    action!(
        "session.refresh",
        "Refresh Sessions",
        "Session",
        "reload sync sessions reconnect",
        [(&["ctrl+shift+r"], None)]
    ),
    action!(
        "session.collapse",
        "Collapse Session",
        "Session",
        "collapse fold sidebar tabs",
        []
    ),
    action!(
        "session.expand",
        "Expand Session",
        "Session",
        "expand unfold sidebar tabs",
        []
    ),
    action!(
        "session.collapse_others",
        "Collapse Other Sessions",
        "Session",
        "collapse other sessions sidebar",
        []
    ),
    action!(
        "client.host.switch",
        "Switch Host",
        "Host",
        "host remote machine server switch",
        [],
        args = HOST
    ),
    action!(
        "client.host.add",
        "Add Remote Host…",
        "Host",
        "host remote add server machine url https connect",
        []
    ),
    action!(
        "client.host.remove",
        "Remove Host…",
        "Host",
        "remove forget delete host remote server",
        []
    ),
    // Tabs
    action!(
        "client.tab.new",
        "New Tab",
        "Tab",
        "create tab window add",
        [(&["ctrl+shift+t"], None)]
    ),
    action!(
        "client.tab.close",
        "Close Tab",
        "Tab",
        "close tab window",
        [(&["ctrl+alt+w"], None)]
    ),
    action!(
        "client.tab.rename",
        "Rename Tab",
        "Tab",
        "rename tab window name title",
        []
    ),
    action!(
        "client.tab.close_others",
        "Close Other Tabs",
        "Tab",
        "close other tabs keep",
        []
    ),
    action!(
        "client.tab.next",
        "Show Next Tab",
        "Tab",
        "cycle tab forward",
        [(&["ctrl+tab"], None), (&["ctrl+pagedown"], None)]
    ),
    action!(
        "client.tab.previous",
        "Show Previous Tab",
        "Tab",
        "cycle tab back",
        [(&["ctrl+shift+tab"], None), (&["ctrl+pageup"], None)]
    ),
    action!(
        "client.tab.goto",
        "Go to Tab",
        "Tab",
        "tab select number index",
        [
            (&["alt+1"], Some(r#"{"index":1}"#)),
            (&["alt+2"], Some(r#"{"index":2}"#)),
            (&["alt+3"], Some(r#"{"index":3}"#)),
            (&["alt+4"], Some(r#"{"index":4}"#)),
            (&["alt+5"], Some(r#"{"index":5}"#)),
            (&["alt+6"], Some(r#"{"index":6}"#)),
            (&["alt+7"], Some(r#"{"index":7}"#)),
            (&["alt+8"], Some(r#"{"index":8}"#)),
            (&["alt+9"], Some(r#"{"index":9}"#)),
        ],
        args = INDEX
    ),
    action!(
        "client.tab.move",
        "Move Tab",
        "Tab",
        "move tab reorder",
        [],
        args = INDEX
    ),
    action!(
        "client.tab.move.backward",
        "Move Tab Backward",
        "Tab",
        "move tab reorder left earlier",
        [(&["ctrl+shift+pageup"], None)]
    ),
    action!(
        "client.tab.move.forward",
        "Move Tab Forward",
        "Tab",
        "move tab reorder right later",
        [(&["ctrl+shift+pagedown"], None)]
    ),
    // Panes
    action!(
        "pane.split",
        "Split Pane",
        "Pane",
        "pane split divide new",
        [],
        args = SPLIT
    ),
    action!(
        "pane.split.left",
        "Split Pane Left",
        "Pane",
        "pane split left",
        []
    ),
    action!(
        "pane.split.right",
        "Split Pane Right",
        "Pane",
        "pane split right",
        [(&["ctrl+shift+o"], None)]
    ),
    action!(
        "pane.split.up",
        "Split Pane Up",
        "Pane",
        "pane split above up",
        []
    ),
    action!(
        "pane.split.down",
        "Split Pane Down",
        "Pane",
        "pane split below down",
        [(&["ctrl+shift+e"], None)]
    ),
    action!(
        "pane.split.auto",
        "Split Pane Best Fit",
        "Pane",
        "pane split automatic best fit",
        [(&["ctrl+alt+enter"], None)]
    ),
    action!(
        "pane.focus",
        "Focus Pane",
        "Pane",
        "pane focus navigate",
        [],
        args = DIRECTION
    ),
    action!(
        "pane.focus.left",
        "Focus Pane Left",
        "Pane",
        "pane focus left",
        [(&["ctrl+alt+left"], None)]
    ),
    action!(
        "pane.focus.right",
        "Focus Pane Right",
        "Pane",
        "pane focus right",
        [(&["ctrl+alt+right"], None)]
    ),
    action!(
        "pane.focus.up",
        "Focus Pane Up",
        "Pane",
        "pane focus up above",
        [(&["ctrl+alt+up"], None)]
    ),
    action!(
        "pane.focus.down",
        "Focus Pane Down",
        "Pane",
        "pane focus down below",
        [(&["ctrl+alt+down"], None)]
    ),
    action!(
        "pane.focus_next",
        "Focus Next Pane",
        "Pane",
        "pane focus next",
        [(&["ctrl+shift+]"], None)]
    ),
    action!(
        "pane.focus_previous",
        "Focus Previous Pane",
        "Pane",
        "pane focus previous",
        [(&["ctrl+shift+["], None)]
    ),
    action!(
        "pane.move",
        "Move Pane",
        "Pane",
        "pane move swap",
        [],
        args = DIRECTION
    ),
    action!(
        "pane.move.left",
        "Move Pane Left",
        "Pane",
        "pane move swap left",
        [(&["ctrl+alt+shift+left"], None)]
    ),
    action!(
        "pane.move.right",
        "Move Pane Right",
        "Pane",
        "pane move swap right",
        [(&["ctrl+alt+shift+right"], None)]
    ),
    action!(
        "pane.move.up",
        "Move Pane Up",
        "Pane",
        "pane move swap up",
        []
    ),
    action!(
        "pane.move.down",
        "Move Pane Down",
        "Pane",
        "pane move swap down",
        []
    ),
    action!(
        "pane.float",
        "Float Pane",
        "Pane",
        "pane float dock detach",
        [(&["ctrl+alt+shift+f"], None)]
    ),
    action!(
        "pane.new_floating",
        "New Floating Pane",
        "Pane",
        "pane new floating scratchpad",
        []
    ),
    action!(
        "pane.minimize_floating",
        "Minimize Floating Window",
        "Pane",
        "floating minimize restore",
        [(&["ctrl+alt+m"], None)]
    ),
    action!(
        "pane.minimize_all_floating",
        "Minimize Floating Windows",
        "Pane",
        "floating all minimize restore",
        [(&["ctrl+alt+shift+m"], None)]
    ),
    action!(
        "pane.zoom",
        "Zoom Pane",
        "Pane",
        "pane zoom maximize restore",
        [(&["ctrl+shift+enter"], None)]
    ),
    action!("pane.close", "Close Pane", "Pane", "pane close remove", []),
    action!(
        "pane.close_others",
        "Close Other Panes",
        "Pane",
        "close other panes keep focused",
        []
    ),
    action!(
        "pane.balance",
        "Balance Splits",
        "Pane",
        "balance equalize evenly",
        []
    ),
    action!(
        "pane.resize",
        "Resize Pane",
        "Pane",
        "pane resize grow shrink divider",
        [],
        args = RESIZE,
        repeats = true
    ),
    action!(
        "pane.move_to_new_tab",
        "Move Pane to New Tab",
        "Pane",
        "pane move new tab detach",
        []
    ),
    action!(
        "pane.move_to_tab",
        "Move Pane to Tab…",
        "Pane",
        "pane move tab send other existing join",
        []
    ),
    action!(
        "pane.go_to_directory",
        "Go to Directory…",
        "Pane",
        "go to directory folder path cd new tab",
        [(&["ctrl+alt+g"], None)]
    ),
    action!(
        "pane.go_to",
        "Go to Anything…",
        "Pane",
        "go to anything pane tab session jump switch find",
        [(&["ctrl+shift+space"], None)]
    ),
    action!(
        "pane.view_activity",
        "View Activity…",
        "Pane",
        "view activity working blocked failed done agents status",
        []
    ),
    action!(
        "pane.insert_path",
        "Insert Path…",
        "Pane",
        "insert path file folder directory cursor type",
        [(&["ctrl+alt+i"], None)]
    ),
    action!(
        "pane.rename",
        "Rename Pane",
        "Pane",
        "rename pane block name title label",
        []
    ),
    action!(
        "pane.copy_id",
        "Copy Pane ID",
        "Pane",
        "pane block id copy",
        []
    ),
    action!(
        "pane.paste",
        "Paste into Pane",
        "Pane",
        "paste clipboard",
        [(&["ctrl+shift+v"], None), (&["shift+insert"], None)]
    ),
    action!(
        "pane.copy",
        "Copy from Pane",
        "Pane",
        "copy selection clipboard",
        [(&["ctrl+shift+c"], None)]
    ),
    action!(
        "pane.select_all",
        "Select All in Pane",
        "Pane",
        "select all scrollback",
        [(&["ctrl+shift+a"], None)]
    ),
    action!(
        "pane.send_key",
        "Send Key",
        "Pane",
        "send key terminal press",
        [],
        args = KEY
    ),
    action!(
        "client.prompt.previous",
        "Jump to Previous Prompt",
        "Pane",
        "jump previous prompt",
        [(&["ctrl+shift+up"], None)]
    ),
    action!(
        "client.prompt.next",
        "Jump to Next Prompt",
        "Pane",
        "jump next prompt",
        [(&["ctrl+shift+down"], None)]
    ),
    action!(
        "client.scroll.page_up",
        "Scroll Page Up",
        "Pane",
        "scroll page up scrollback",
        [(&["shift+pageup"], None)],
        repeats = true
    ),
    action!(
        "client.scroll.page_down",
        "Scroll Page Down",
        "Pane",
        "scroll page down scrollback",
        [(&["shift+pagedown"], None)],
        repeats = true
    ),
    // View
    action!(
        "client.font.increase",
        "Increase Font Size",
        "View",
        "font size larger zoom",
        [
            (&["ctrl+="], None),
            (&["ctrl++"], None),
            (&["ctrl+shift+="], None)
        ],
        repeats = true
    ),
    action!(
        "client.font.decrease",
        "Decrease Font Size",
        "View",
        "font size smaller zoom",
        [(&["ctrl+-"], None)],
        repeats = true
    ),
    action!(
        "client.font.reset",
        "Reset Font Size",
        "View",
        "font size reset default",
        [(&["ctrl+0"], None)]
    ),
    action!(
        "client.sidebar.toggle",
        "Toggle Sidebar",
        "View",
        "sidebar collapse expand hide show",
        [(&["ctrl+shift+b"], None)]
    ),
    action!(
        "client.toolbar.reveal",
        "Reveal Toolbar",
        "View",
        "toolbar tabs show reveal hidden hover",
        [(&["ctrl+alt+shift+t"], None)]
    ),
    action!(
        "client.pane_header.reveal",
        "Reveal Pane Header",
        "View",
        "pane header title show reveal hidden hover",
        [(&["ctrl+alt+shift+h"], None)]
    ),
    action!(
        "client.tab_style.toggle",
        "Toggle Toolbar Layout",
        "View",
        "toolbar layout tabs vertical horizontal sidebar above left right",
        [(&["ctrl+shift+s"], None)]
    ),
    action!(
        "client.sidebar.filter.focus",
        "Focus Tab Filter",
        "View",
        "filter search find type query focus keyboard sidebar tabs sessions hosts",
        []
    ),
    action!(
        "client.sidebar.activity.toggle",
        "Toggle Activity View",
        "View",
        "activity sidebar attention blocked done programs filter",
        [(&["ctrl+shift+."], None)]
    ),
    action!(
        "client.sidebar.activity.by_session",
        "Group Activity by Session",
        "View",
        "activity sidebar group session",
        []
    ),
    action!(
        "client.sidebar.activity.by_priority",
        "Sort Activity by Priority",
        "View",
        "activity sidebar sort priority urgency",
        []
    ),
];

pub fn find(name: &str) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|a| a.name == name)
}

/// The `client.keymap` request: rexia's actions and its default bindings as the bottom layer,
/// beneath init.lua and client scripts.
pub fn keymap_request() -> Value {
    let actions: Vec<Value> = ACTIONS
        .iter()
        .map(|a| {
            let mut action = json!({"name": a.name, "kind": "client", "title": a.title,
                "category": a.category, "keywords": a.keywords});
            if let Some(schema) = a.args {
                action["args"] = serde_json::from_str(schema).expect("valid schema");
            }
            if a.repeats {
                action["repeats"] = json!(true);
            }
            action
        })
        .collect();
    let bindings: Vec<Value> = ACTIONS
        .iter()
        .flat_map(|a| {
            a.keys.iter().map(|(keys, args)| {
                let mut binding = json!({"keys": keys, "action": a.name});
                if let Some(args) = args {
                    binding["args"] = serde_json::from_str(args).expect("valid args");
                }
                binding
            })
        })
        .collect();
    json!({"actions": actions, "defaults": [{"name": "rexia", "bindings": bindings}]})
}

/// rexia's default bindings alone, for before the server's keymap arrives.
pub fn fallback_keymap() -> crate::keymap::Keymap {
    let request = keymap_request();
    crate::keymap::Keymap::parse(&json!({
        "bindings": request["defaults"][0]["bindings"],
        "actions": request["actions"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fallback_keymap_binds_the_palette() {
        let keymap = fallback_keymap();
        assert!(
            keymap
                .bindings
                .iter()
                .any(|b| b.action == "client.palette.commands" || b.keys == ["ctrl+shift+p"])
        );
    }

    #[test]
    fn names_are_unique_and_default_keys_do_not_collide() {
        let mut names = HashSet::new();
        let mut keys = HashSet::new();
        for action in ACTIONS {
            assert!(
                names.insert(action.name),
                "duplicate action {}",
                action.name
            );
            for (binding, args) in action.keys {
                let key = (binding.join(" "), *args);
                assert!(
                    keys.insert(binding.join(" ")),
                    "{} is bound twice ({:?})",
                    key.0,
                    key.1
                );
            }
        }
        assert!(find("pane.split.right").is_some());
    }

    #[test]
    fn request_lists_actions_with_schemas_and_bindings() {
        let request = keymap_request();
        let actions = request["actions"].as_array().unwrap();
        assert_eq!(actions.len(), ACTIONS.len());
        let goto = actions
            .iter()
            .find(|a| a["name"] == "client.tab.goto")
            .unwrap();
        assert_eq!(goto["args"]["required"][0], "index");
        let bindings = request["defaults"][0]["bindings"].as_array().unwrap();
        assert!(
            bindings
                .iter()
                .any(|b| b["keys"][0] == "alt+3" && b["args"]["index"] == 3)
        );
    }
}
