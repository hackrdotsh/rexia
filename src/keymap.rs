//! The keymap the server resolves for this client (`client.keymap`): rexia's defaults,
//! then init.lua, then client scripts. rexia runs it itself so key sequences and modes work.
use serde_json::Value;
use std::collections::HashMap;

/// One keystroke as a modifier set plus the key, before spelling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stroke {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// The key as the layout produced it, such as `t`, `}` or `pageup`.
    pub key: String,
    /// The character the key types without shift, when the layout needed shift for `key`.
    pub unshifted: Option<String>,
    /// The W3C code of the physical key, such as `KeyT`.
    pub code: Option<String>,
}

fn spell(ctrl: bool, alt: bool, shift: bool, cmd: bool, key: &str) -> String {
    let mut out = String::new();
    for (on, name) in [
        (ctrl, "ctrl+"),
        (alt, "alt+"),
        (shift, "shift+"),
        (cmd, "cmd+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(key);
    out
}

impl Stroke {
    /// Every spelling a binding could use for this stroke: by layout, by layout with the
    /// shift a symbol needed, and by physical key.
    pub fn spellings(&self) -> Vec<String> {
        let mut out = vec![spell(self.ctrl, self.alt, self.shift, self.cmd, &self.key)];
        if let Some(base) = &self.unshifted {
            out.push(spell(self.ctrl, self.alt, true, self.cmd, base));
        }
        if let Some(code) = &self.code {
            let shift = self.shift || self.unshifted.is_some();
            out.push(spell(
                self.ctrl,
                self.alt,
                shift,
                self.cmd,
                &format!("[{code}]"),
            ));
        }
        out.dedup();
        out
    }

    pub fn is_escape(&self) -> bool {
        self.key == "esc" && !self.ctrl && !self.alt && !self.cmd
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub keys: Vec<String>,
    pub action: String,
    pub args: Option<Value>,
    pub mode: Option<String>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mode {
    pub exclusive: bool,
    pub blocked: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerAction {
    pub name: String,
    pub kind: String,
    pub title: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    pub bindings: Vec<Binding>,
    pub modes: HashMap<String, Mode>,
    pub actions: Vec<ServerAction>,
    pub diagnostics: Vec<String>,
    pub config_path: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Lookup<'k> {
    /// Run this binding.
    Run(&'k Binding),
    /// The keys so far start a longer sequence; wait for the next key.
    Prefix,
    /// The active mode swallows keys it doesn't bind.
    Swallow,
    /// Nothing is bound: the key goes to the terminal.
    Unbound,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

impl Keymap {
    pub fn parse(value: &Value) -> Self {
        let text = |v: &Value| v.as_str().map(str::to_owned);
        let bindings = value["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| Binding {
                keys: strings(&b["keys"]),
                action: b["action"].as_str().unwrap_or_default().to_owned(),
                args: (!b["args"].is_null()).then(|| b["args"].clone()),
                mode: text(&b["mode"]),
                source: text(&b["source"]),
            })
            .filter(|b| !b.keys.is_empty() && !b.action.is_empty())
            .collect();
        let modes = value["modes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some((
                    m["name"].as_str()?.to_owned(),
                    Mode {
                        exclusive: m["exclusive"].as_bool().unwrap_or(false),
                        blocked: m["blocked"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(strings)
                            .collect(),
                    },
                ))
            })
            .collect();
        let actions = value["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some(ServerAction {
                    name: a["name"].as_str()?.to_owned(),
                    kind: a["kind"].as_str().unwrap_or("lua").to_owned(),
                    title: text(&a["title"]),
                    category: text(&a["category"]),
                    description: text(&a["description"]),
                    keywords: text(&a["keywords"]),
                })
            })
            .collect();
        let diagnostics = value["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let message = d["message"].as_str()?;
                Some(match d["source"].as_str() {
                    Some(source) => format!("{source}: {message}"),
                    None => message.to_owned(),
                })
            })
            .collect();
        Self {
            bindings,
            modes,
            actions,
            diagnostics,
            config_path: text(&value["config"]["path"]),
        }
    }

    fn matches(binding: &Binding, typed: &[Vec<String>]) -> bool {
        binding.keys.len() >= typed.len()
            && binding
                .keys
                .iter()
                .zip(typed)
                .all(|(key, spellings)| spellings.iter().any(|s| s == key))
    }

    /// What the keys typed so far (each as its spellings) do in `mode`, trying bindings in
    /// the server's order and skipping actions `available` rejects.
    pub fn lookup(
        &self,
        mode: Option<&str>,
        typed: &[Vec<String>],
        available: impl Fn(&str) -> bool,
    ) -> Lookup<'_> {
        let in_scope = |b: &&Binding| b.mode.as_deref() == mode;
        let mut prefix = false;
        for binding in self.bindings.iter().filter(in_scope) {
            if !Self::matches(binding, typed) {
                continue;
            }
            if binding.keys.len() == typed.len() {
                if available(&binding.action) {
                    return Lookup::Run(binding);
                }
            } else {
                prefix = true;
            }
        }
        if prefix {
            return Lookup::Prefix;
        }
        if let Some(mode) = mode.and_then(|m| self.modes.get(m)) {
            let blocked = typed.len() == 1
                && mode
                    .blocked
                    .iter()
                    .any(|keys| keys.len() == 1 && typed[0].contains(&keys[0]));
            if mode.exclusive || blocked {
                return Lookup::Swallow;
            }
        }
        Lookup::Unbound
    }

    /// The bindings that could follow the keys typed so far, for a hint.
    pub fn continuations(&self, mode: Option<&str>, typed: &[Vec<String>]) -> Vec<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.mode.as_deref() == mode && b.keys.len() > typed.len())
            .filter(|b| Self::matches(b, typed))
            .collect()
    }

    /// The keys bound to an action outside every mode, for menus and the palette.
    pub fn keys_for(&self, action: &str, args: Option<&Value>) -> Option<&[String]> {
        self.bindings
            .iter()
            .find(|b| {
                b.mode.is_none()
                    && b.action == action
                    && (args.is_none() || b.args.as_ref() == args)
            })
            .map(|b| b.keys.as_slice())
    }
}

/// A binding for display, such as `Ctrl+Shift+T` or `Ctrl+A C`.
pub fn display(keys: &[String]) -> String {
    keys.iter()
        .map(|key| {
            let mut rest = key.as_str();
            let mut parts = Vec::new();
            for (prefix, name) in [
                ("ctrl+", "Ctrl"),
                ("alt+", "Alt"),
                ("shift+", "Shift"),
                ("cmd+", "Super"),
            ] {
                if let Some(after) = rest.strip_prefix(prefix) {
                    parts.push(name.to_owned());
                    rest = after;
                }
            }
            let mut chars = rest.chars();
            parts.push(match rest {
                "pageup" => "PageUp".to_owned(),
                "pagedown" => "PageDown".to_owned(),
                _ => match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                },
            });
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stroke(ctrl: bool, shift: bool, key: &str) -> Stroke {
        Stroke {
            ctrl,
            shift,
            key: key.into(),
            ..Default::default()
        }
    }

    fn keymap() -> Keymap {
        Keymap::parse(&json!({
            "bindings": [
                {"keys": ["ctrl+shift+t"], "action": "client.tab.new", "source": "rexia"},
                {"keys": ["ctrl+a", "c"], "action": "client.tab.new", "source": "init.lua:3"},
                {"keys": ["ctrl+a", "x"], "action": "client.key.forward", "source": "init.lua:4"},
                {"keys": ["ctrl+shift+]"], "action": "pane.focus_next"},
                {"keys": ["ctrl+[Digit1]"], "action": "client.tab.goto", "args": {"index": 1}},
                {"keys": ["left"], "action": "pane.resize", "args": {"direction": "left"}, "mode": "resize"},
                {"keys": ["esc"], "action": "client.mode.exit", "mode": "resize"},
                {"keys": ["ctrl+e"], "action": "missing.action"},
                {"keys": ["ctrl+e"], "action": "pane.zoom"}
            ],
            "modes": [{"name": "resize", "exclusive": true}, {"name": "copy", "exclusive": false, "blocked": [["q"]]}],
            "actions": [{"name": "dev_layout", "kind": "lua", "title": "Dev Layout"}],
            "diagnostics": [{"severity": "warning", "source": "init.lua:9", "message": "unknown action"}],
            "config": {"path": "/home/me/.config/rex/init.lua"}
        }))
    }

    #[test]
    fn spellings_cover_layout_shift_and_physical_keys() {
        let brace = Stroke {
            ctrl: true,
            key: "}".into(),
            unshifted: Some("]".into()),
            code: Some("BracketRight".into()),
            ..Default::default()
        };
        assert_eq!(
            brace.spellings(),
            vec!["ctrl+}", "ctrl+shift+]", "ctrl+shift+[BracketRight]"]
        );
        let all = Stroke {
            ctrl: true,
            alt: true,
            shift: true,
            cmd: true,
            key: "x".into(),
            ..Default::default()
        };
        assert_eq!(all.spellings(), vec!["ctrl+alt+shift+cmd+x"]);
    }

    #[test]
    fn single_keys_sequences_and_availability() {
        let map = keymap();
        let all = |_: &str| true;
        let t = vec![stroke(true, true, "t").spellings()];
        assert!(
            matches!(map.lookup(None, &t, all), Lookup::Run(b) if b.action == "client.tab.new")
        );
        let a = vec![stroke(true, false, "a").spellings()];
        assert_eq!(map.lookup(None, &a, all), Lookup::Prefix);
        assert_eq!(map.continuations(None, &a).len(), 2);
        let ac = vec![a[0].clone(), stroke(false, false, "c").spellings()];
        assert!(
            matches!(map.lookup(None, &ac, all), Lookup::Run(b) if b.source.as_deref() == Some("init.lua:3"))
        );
        let az = vec![a[0].clone(), stroke(false, false, "z").spellings()];
        assert_eq!(map.lookup(None, &az, all), Lookup::Unbound);
        let e = vec![stroke(true, false, "e").spellings()];
        let known = |action: &str| action != "missing.action";
        assert!(matches!(map.lookup(None, &e, known), Lookup::Run(b) if b.action == "pane.zoom"));
        let one = Stroke {
            ctrl: true,
            key: "1".into(),
            code: Some("Digit1".into()),
            ..Default::default()
        };
        assert!(
            matches!(map.lookup(None, &[one.spellings()], all), Lookup::Run(b) if b.args == Some(json!({"index": 1})))
        );
    }

    #[test]
    fn modes_scope_bindings_and_swallow_or_block_keys() {
        let map = keymap();
        let all = |_: &str| true;
        let left = vec![stroke(false, false, "left").spellings()];
        assert!(
            matches!(map.lookup(Some("resize"), &left, all), Lookup::Run(b) if b.action == "pane.resize")
        );
        assert_eq!(map.lookup(None, &left, all), Lookup::Unbound);
        let x = vec![stroke(false, false, "x").spellings()];
        assert_eq!(map.lookup(Some("resize"), &x, all), Lookup::Swallow);
        let q = vec![stroke(false, false, "q").spellings()];
        assert_eq!(map.lookup(Some("copy"), &q, all), Lookup::Swallow);
        assert_eq!(map.lookup(Some("copy"), &x, all), Lookup::Unbound);
    }

    #[test]
    fn parses_metadata_and_displays_keys() {
        let map = keymap();
        assert_eq!(map.actions[0].name, "dev_layout");
        assert_eq!(map.diagnostics, vec!["init.lua:9: unknown action"]);
        assert_eq!(
            map.config_path.as_deref(),
            Some("/home/me/.config/rex/init.lua")
        );
        assert_eq!(
            map.keys_for("pane.focus_next", None),
            Some(&["ctrl+shift+]".to_owned()][..])
        );
        assert_eq!(display(&["ctrl+shift+t".into()]), "Ctrl+Shift+T");
        assert_eq!(display(&["ctrl+a".into(), "c".into()]), "Ctrl+A C");
        assert_eq!(display(&["ctrl++".into()]), "Ctrl++");
        assert_eq!(display(&["ctrl+shift+pageup".into()]), "Ctrl+Shift+PageUp");
    }
}
