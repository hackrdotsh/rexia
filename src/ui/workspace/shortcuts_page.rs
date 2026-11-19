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
    pub(super) fn mode_shortcut_binding(
        &self,
        binding: &actions::ModeShortcut,
    ) -> Option<&keymap::Binding> {
        let keys = self.settings.shortcuts.get(&binding.id());
        let args: Option<serde_json::Value> = binding
            .args
            .map(|args| serde_json::from_str(args).expect("valid binding args"));
        self.keymap.bindings.iter().find(|row| {
            let correct_keys = keys.map_or(row.keys == binding.keys, |keys| &row.keys == keys);
            row.mode.as_deref() == Some(binding.mode)
                && correct_keys
                && row.action == binding.action.name
                && (row.args == args
                    || (args.is_none()
                        && row
                            .args
                            .as_ref()
                            .and_then(serde_json::Value::as_object)
                            .is_some_and(|args| args.is_empty())))
        })
    }

    /// Watch the containing directory so atomic replacement by an editor still works.
    pub(super) fn watch_shortcuts(
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<(::notify::RecommendedWatcher, gpui::Task<()>)> {
        use ::notify::Watcher as _;
        let directory = path.parent()?.to_owned();
        std::fs::create_dir_all(&directory).ok()?;
        let (send, changes) = flume::bounded(1);
        let watched = path.clone();
        let mut watcher =
            ::notify::recommended_watcher(move |event: ::notify::Result<::notify::Event>| {
                if let Ok(event) = event
                    && (event.kind.is_modify() || event.kind.is_create())
                    && event.paths.contains(&watched)
                {
                    let _ = send.try_send(());
                }
            })
            .ok()?;
        watcher
            .watch(&directory, ::notify::RecursiveMode::NonRecursive)
            .ok()?;
        let task = cx.spawn(async move |this, cx| {
            while changes.recv_async().await.is_ok() {
                let read_path = path.clone();
                let Ok(settings) = cx
                    .background_executor()
                    .spawn(async move { crate::settings::Settings::load(&read_path) })
                    .await
                else {
                    continue;
                };
                if this
                    .update(cx, |ws, cx| {
                        if ws.settings.shortcuts != settings.shortcuts {
                            ws.settings.shortcuts = settings.shortcuts;
                            ws.keymap = actions::fallback_keymap(&ws.settings.shortcuts);
                            ws.send_keymap();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Some((watcher, task))
    }

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
        let mut rows: Vec<ShortcutRow> = actions::all()
            .filter(|a| a.name != "client.key.forward")
            .filter_map(|a| {
                let (keys, config) = keys_of(a.name);
                if a.mode.is_some() && config.is_none() {
                    return None;
                }
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
                Some(ShortcutRow {
                    action: a.name.to_owned(),
                    title: a.title.trim_end_matches('…').to_owned(),
                    keys,
                    source,
                    config,
                    editable: actions::rebindable(a),
                    changed,
                })
            })
            .collect();
        rows.extend(actions::mode_shortcuts().map(|binding| {
            let action = binding.id();
            let changed = self.settings.shortcuts.contains_key(&action);
            let effective = self.mode_shortcut_binding(&binding);
            let shown =
                effective.map(|row| format!("{}: {}", binding.mode, keymap::display(&row.keys)));
            let source = match effective.and_then(|row| row.source.as_deref()) {
                Some(file) if file.contains(".lua") => Source::Config(file.to_owned()),
                _ if shown.is_some() && changed => Source::Yours,
                _ if shown.is_some() => Source::Default,
                _ => Source::None,
            };
            ShortcutRow {
                action,
                title: binding.title(),
                keys: shown.into_iter().collect(),
                source,
                config: None,
                editable: true,
                changed,
            }
        }));
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
            if let Some(binding) = actions::ModeShortcut::find(name) {
                let mode = binding.mode;
                let action = binding.action.name;
                let default = format!("{mode}/{}", binding.keys.join(">"));
                lines.push(format!("rex.unbind({default:?}, {action:?})"));
                if !keys.is_empty() {
                    let keys = format!("{mode}/{}", keys.join(">"));
                    let args = binding.args.map(lua_binding_args).unwrap_or_default();
                    lines.push(format!("rex.bind({keys:?}, {action:?}{args})"));
                }
                continue;
            }
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
        if let Some(binding) = actions::ModeShortcut::find(action) {
            let taken = self
                .settings
                .shortcuts
                .iter()
                .find(|(id, keys)| {
                    id.as_str() != action
                        && keys.as_slice() == binding.keys
                        && actions::ModeShortcut::find(id)
                            .is_some_and(|other| other.mode == binding.mode)
                })
                .map(|(id, _)| id.clone());
            self.reset_now(action, taken, cx);
            return;
        }
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

fn lua_binding_args(args: &str) -> String {
    let values: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(args).expect("valid binding args");
    let parts = values
        .iter()
        .map(|(key, value)| format!("{key} = {value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(", {{ {parts} }}")
}
