//! Settings › Shortcuts: every action with its keys and where they come from (rexia's
//! defaults, your changes, or init.lua), filtered as you type. Record a new key, remove a
//! key or reset to the default from each row; keys bound to more than one action are
//! called out, the server's keymap problems are listed, and Copy as Lua gives your changes
//! as `rex.bind`/`rex.unbind` lines for init.lua.
use super::{
    Workspace,
    overlay::Confirm,
    preferences::{button, row},
};
use crate::{
    keymap,
    ui::{actions, theme::Theme},
};
use gpui::{
    AnyElement, ClipboardItem, Context, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::*, px,
};
use std::collections::BTreeMap;

/// Where an action's keys come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// rexia's own default.
    Default,
    /// Changed in rexia (Change Shortcut or this page).
    Yours,
    /// The server's configuration file, at this line.
    Config(String),
    /// No key.
    None,
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Self::Default => "Default".into(),
            Self::Yours => "Yours".into(),
            Self::Config(at) => at.clone(),
            Self::None => "No key".into(),
        }
    }
}

/// One action on the page.
#[derive(Clone, Debug)]
pub struct ShortcutRow {
    pub action: String,
    pub title: String,
    /// Each binding's keys as shown, with its mode first when it has one.
    pub keys: Vec<String>,
    pub source: Source,
    /// The init.lua line that binds it too, alongside rexia's own keys.
    pub config: Option<String>,
    /// rexia can change it (its own actions without fixed arguments).
    pub editable: bool,
    /// You changed it, so Reset applies.
    pub changed: bool,
}

impl Workspace {
    /// Every action, rexia's then the server's, with its keys, matching `filter` (title,
    /// name, category or keys).
    pub fn shortcut_rows(&self, filter: &str) -> Vec<ShortcutRow> {
        let filter = filter.trim().to_lowercase();
        let keys_of = |name: &str| -> (Vec<String>, Option<String>) {
            let mut config = None;
            let keys = self
                .keymap
                .bindings
                .iter()
                .filter(|b| b.action == name)
                .map(|b| {
                    if config.is_none()
                        && let Some(source) = b.source.as_deref().filter(|s| s.contains(".lua"))
                    {
                        config = Some(source.to_owned());
                    }
                    let keys = keymap::display(&b.keys);
                    match &b.mode {
                        Some(mode) => format!("{mode}: {keys}"),
                        None => keys,
                    }
                })
                .collect();
            (keys, config)
        };
        let mut rows: Vec<ShortcutRow> = actions::ACTIONS
            .iter()
            .filter(|a| a.name != "client.key.forward")
            .map(|a| {
                let (keys, config) = keys_of(a.name);
                let changed = self.settings.shortcuts.contains_key(a.name);
                // rexia's own keys come first; init.lua's are noted beside them.
                let own = self
                    .settings
                    .shortcuts
                    .get(a.name)
                    .map_or(!a.keys.is_empty(), |k| !k.is_empty());
                let source = match (changed, own, &config) {
                    (true, true, _) => Source::Yours,
                    (false, true, _) => Source::Default,
                    (_, false, Some(at)) => Source::Config(at.clone()),
                    (_, false, None) => Source::None,
                };
                let config = config.filter(|_| own);
                ShortcutRow {
                    action: a.name.to_owned(),
                    title: a.title.trim_end_matches('…').to_owned(),
                    keys,
                    source,
                    config,
                    editable: actions::rebindable(a),
                    changed,
                }
            })
            .collect();
        // Actions the server's configuration defines, such as Lua actions.
        for action in &self.keymap.actions {
            if actions::find(&action.name).is_some() || rows.iter().any(|r| r.action == action.name)
            {
                continue;
            }
            let (keys, config) = keys_of(&action.name);
            rows.push(ShortcutRow {
                action: action.name.clone(),
                title: action.title.clone().unwrap_or_else(|| action.name.clone()),
                source: config.map_or(Source::None, Source::Config),
                config: None,
                keys,
                editable: false,
                changed: false,
            });
        }
        if filter.is_empty() {
            return rows;
        }
        rows.into_iter()
            .filter(|r| {
                let category = actions::find(&r.action).map_or("", |a| a.category);
                [r.title.as_str(), r.action.as_str(), category]
                    .iter()
                    .any(|t| t.to_lowercase().contains(&filter))
                    || r.keys.iter().any(|k| k.to_lowercase().contains(&filter))
            })
            .collect()
    }

    /// Keys bound to more than one action in the same mode, as "keys: Action, Action".
    pub fn shortcut_conflicts(&self) -> Vec<String> {
        let mut by_keys: BTreeMap<(Option<String>, Vec<String>), Vec<String>> = BTreeMap::new();
        for binding in &self.keymap.bindings {
            let names = by_keys
                .entry((binding.mode.clone(), binding.keys.clone()))
                .or_default();
            if !names.contains(&binding.action) {
                names.push(binding.action.clone());
            }
        }
        by_keys
            .into_iter()
            .filter(|(_, names)| names.len() > 1)
            .map(|((mode, keys), names)| {
                let titles: Vec<String> = names
                    .iter()
                    .map(|n| {
                        actions::find(n)
                            .map_or(n.clone(), |a| a.title.trim_end_matches('…').to_owned())
                    })
                    .collect();
                let keys = keymap::display(&keys);
                match mode {
                    Some(mode) => format!("{mode}: {keys}: {}", titles.join(", ")),
                    None => format!("{keys}: {}", titles.join(", ")),
                }
            })
            .collect()
    }

    /// Your changes as init.lua lines: defaults you replaced or removed are unbound, and
    /// new keys bound.
    pub fn shortcuts_as_lua(&self) -> String {
        let mut lines = vec!["-- rexia's shortcut changes, for ~/.config/rex/init.lua".to_owned()];
        for (name, keys) in &self.settings.shortcuts {
            let Some(action) = actions::find(name) else {
                continue;
            };
            for (default, _) in action.keys {
                if keys.as_slice() != default.iter().map(|k| k.to_string()).collect::<Vec<_>>() {
                    lines.push(format!("rex.unbind({:?}, {name:?})", default.join(" ")));
                }
            }
            if !keys.is_empty() {
                lines.push(format!("rex.bind({:?}, {name:?})", keys.join(" ")));
            }
        }
        lines.join("\n") + "\n"
    }

    /// Give an action no key.
    pub fn remove_shortcut(&mut self, action: &str, cx: &mut Context<Self>) {
        let action = action.to_owned();
        self.update_settings(|s| drop(s.shortcuts.insert(action, Vec::new())), cx);
        self.send_keymap();
    }

    /// Put an action's default keys back. When another action has taken one of them,
    /// ask first: resetting takes the key back from it.
    pub fn reset_shortcut(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(def) = actions::find(action) else {
            return;
        };
        let defaults: Vec<Vec<String>> = def
            .keys
            .iter()
            .map(|(keys, _)| keys.iter().map(|k| k.to_string()).collect())
            .collect();
        let taken: Vec<String> = self
            .settings
            .shortcuts
            .iter()
            .filter(|(name, keys)| *name != action && defaults.contains(keys))
            .map(|(name, _)| name.clone())
            .collect();
        if let Some(other) = taken.first() {
            let other_title = actions::find(other).map_or(other.as_str(), |a| a.title);
            let keys = keymap::display(&self.settings.shortcuts[other]);
            self.open_confirm(
                format!("Reset {}?", def.title.trim_end_matches('…')),
                format!(
                    "{keys} is now {}'s. Resetting gives it back to {} and leaves {} without a key.",
                    other_title.trim_end_matches('…'),
                    def.title.trim_end_matches('…'),
                    other_title.trim_end_matches('…'),
                ),
                "Reset".into(),
                Confirm::ResetShortcut {
                    action: action.to_owned(),
                    from: other.clone(),
                },
                cx,
            );
            // The question takes the keys, and closing it comes back to this page.
            window.focus(&self.focus, cx);
            self.shortcut_return = true;
            return;
        }
        self.reset_now(action, None, cx);
    }

    /// Reset `action`, taking its key back from `from` if another action had it.
    pub(super) fn reset_now(&mut self, action: &str, from: Option<String>, cx: &mut Context<Self>) {
        let action = action.to_owned();
        self.update_settings(
            |s| {
                s.shortcuts.remove(&action);
                if let Some(from) = from {
                    s.shortcuts.insert(from, Vec::new());
                }
            },
            cx,
        );
        self.send_keymap();
    }

    /// Tell every host rexia's keys after a change.
    pub(super) fn send_keymap(&self) {
        let request = actions::keymap_request(&self.settings.shortcuts);
        for host in &self.hosts {
            host.handle
                .send(crate::rex::host::HostCommand::Keymap(request.clone()));
        }
    }

    pub(super) fn shortcuts_section(
        &self,
        filter: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let conflicts = self.shortcut_conflicts();
        let diagnostics = &self.keymap.diagnostics;
        let lua = self.shortcuts_as_lua();
        out.push(
            row(
                "shortcuts-lua",
                "Your changes as Lua",
                "rex.bind and rex.unbind lines for init.lua, so every client gets them.",
                theme,
            )
            .child(
                button("shortcuts-copy-lua", "Copy as Lua", false, theme).on_click(cx.listener(
                    move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(lua.clone()))
                    },
                )),
            )
            .into_any_element(),
        );
        for (n, line) in conflicts.iter().chain(diagnostics.iter()).enumerate() {
            out.push(
                div()
                    .id(SharedString::from(format!("shortcut-warning-{n}")))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(theme.error)
                    .child(if n < conflicts.len() {
                        format!("Two actions share a key: {line}")
                    } else {
                        line.clone()
                    })
                    .into_any_element(),
            );
        }
        for r in self.shortcut_rows(filter) {
            let keys = if r.keys.is_empty() {
                "No key".to_owned()
            } else {
                r.keys.join(", ")
            };
            let mut line = row(
                format!("shortcut-{}", r.action),
                &r.title,
                &match &r.config {
                    Some(at) => format!("{} · {}, and {at}", r.action, r.source.label()),
                    None => format!("{} · {}", r.action, r.source.label()),
                },
                theme,
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(if r.keys.is_empty() {
                        theme.text_muted
                    } else {
                        theme.text
                    })
                    .child(keys),
            );
            if r.editable {
                let (record, title) = (r.action.clone(), r.title.clone());
                line = line.child(
                    button(
                        format!("shortcut-change-{}", r.action),
                        "Change…",
                        false,
                        theme,
                    )
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.record_from_settings(record.clone(), title.clone(), window, cx)
                    })),
                );
                if !r.keys.is_empty() {
                    let action = r.action.clone();
                    line = line.child(
                        button(
                            format!("shortcut-remove-{}", r.action),
                            "Remove",
                            false,
                            theme,
                        )
                        .on_click(cx.listener(move |ws, _, _, cx| ws.remove_shortcut(&action, cx))),
                    );
                }
                if r.changed {
                    let action = r.action.clone();
                    line = line.child(
                        button(
                            format!("shortcut-reset-{}", r.action),
                            "Reset",
                            false,
                            theme,
                        )
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            ws.reset_shortcut(&action, window, cx)
                        })),
                    );
                }
            }
            out.push(line.into_any_element());
        }
        out
    }

    /// Change Shortcut from the Shortcuts page: record the key, then come back here.
    pub fn record_from_settings(
        &mut self,
        action: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_settings_fields(cx);
        self.shortcut_return = true;
        self.record_shortcut(action, title, window, cx);
    }

    /// After recording from the Shortcuts page, show it again.
    pub(super) fn back_to_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.shortcut_return) {
            self.open_settings(window, cx);
            self.show_settings_section(super::SettingsSection::Shortcuts, window, cx);
        }
    }
}
