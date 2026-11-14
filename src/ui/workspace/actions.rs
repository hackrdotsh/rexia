//! Running actions: from keys (through the server's keymap), from other clients
//! (`client.perform`), and from buttons. Actions that belong to the server, such as init.lua
//! actions and block actions, go back to it with `client.action`.
use super::{Workspace, overlay};
use crate::{
    keymap::{Keymap, Lookup},
    rex::{
        host::{HostCommand, PerformRequest, shell_layout},
        view::{self, View},
    },
    ui::{actions, keys, terminal::TerminalPane, titles},
};
use gpui::{ClipboardItem, Context, Entity, KeyDownEvent, Window};
use serde_json::{Value, json};

/// The session and block an action applies to; the focused ones when left out.
#[derive(Clone, Debug, Default)]
pub struct Target {
    pub session_id: Option<String>,
    pub block_id: Option<String>,
}

/// An action's result, or why it wasn't performed: (`not_supported` | `refused` | `internal`,
/// message), as `client.reply` reports it.
pub type Outcome = Result<Value, (String, String)>;

fn refused(message: impl Into<String>) -> (String, String) {
    ("refused".into(), message.into())
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

impl Workspace {
    /// Rebuild the keymap when the server sent a new one.
    pub(super) fn refresh_keymap(&mut self) {
        let value = self
            .host()
            .handle
            .model
            .lock()
            .expect("host model")
            .keymap
            .clone();
        if value.as_ref() != self.keymap_source.as_ref() {
            // Until the server answers, rexia's own defaults apply, so early keys still work.
            self.keymap = value
                .as_ref()
                .map(Keymap::parse)
                .unwrap_or_else(actions::fallback_keymap);
            // A mode the new keymap no longer has can't stay active.
            self.modes.retain(|m| self.keymap.modes.contains_key(m));
            self.keymap_source = value;
        }
    }

    /// Whether a binding to `action` can run here. Server actions always can.
    pub(super) fn available(&self, action: &str) -> bool {
        action == "client.key.forward"
            || actions::find(action).is_some()
            || !action.contains('.')
            || self.keymap.actions.iter().any(|a| a.name == action)
    }

    fn pane_entity(&self, block: &str) -> Option<Entity<TerminalPane>> {
        self.panes.get(block).cloned()
    }

    fn focused_block(&self) -> Option<String> {
        self.model()
            .view()
            .and_then(View::active_window)
            .and_then(|w| w.focused_block_id.clone())
    }

    /// Keys first meet the keymap: a binding runs, a sequence waits for its next key, and
    /// everything else goes on to the focused terminal.
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keys::is_modifier(&event.keystroke) {
            return;
        }
        // Palettes and dialogs take keys first; their fields edit and Esc closes them. The key
        // that opened a list closes it again.
        if let Some(kind) = self.list_kind() {
            let stroke = keys::stroke(&event.keystroke);
            if let Lookup::Run(binding) = self.keymap.lookup(None, &[stroke.spellings()], |_| true)
                && binding.action == kind.action()
            {
                cx.stop_propagation();
                self.close_overlay(window, cx);
            }
            return;
        }
        if self.overlay.is_some() {
            if self.dialog_key(&event.keystroke.key, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if let Some(find) = &self.find
            && gpui::Focusable::focus_handle(find.input.read(cx), cx).is_focused(window)
        {
            return;
        }
        let stroke = keys::stroke(&event.keystroke);
        let mode = self.modes.last().cloned();
        let mut typed = self.pending.clone();
        typed.push(stroke.spellings());
        let available = |action: &str| self.available(action);
        let lookup = self.keymap.lookup(mode.as_deref(), &typed, available);
        let binding = match lookup {
            Lookup::Run(binding) => binding.clone(),
            Lookup::Prefix => {
                self.pending = typed;
                cx.stop_propagation();
                cx.notify();
                return;
            }
            Lookup::Swallow => {
                self.pending.clear();
                if stroke.is_escape() {
                    self.modes.pop();
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
            Lookup::Unbound => {
                let had_prefix = !self.pending.is_empty();
                self.pending.clear();
                if had_prefix {
                    cx.notify();
                }
                // Escape leaves a mode that doesn't bind it.
                if mode.is_some() && stroke.is_escape() && !had_prefix {
                    self.modes.pop();
                    cx.stop_propagation();
                    cx.notify();
                }
                return;
            }
        };
        self.pending.clear();
        cx.stop_propagation();
        if event.is_held && !actions::find(&binding.action).is_some_and(|a| a.repeats) {
            return;
        }
        if binding.action == "client.key.forward" {
            self.forward_key(&event.keystroke, &binding.keys, event.is_held);
            return;
        }
        let args = binding.args.clone().unwrap_or(Value::Null);
        if let Err((_, message)) =
            self.run_action(&binding.action, args, "key", Target::default(), window, cx)
        {
            self.notice = Some(message);
        }
        cx.notify();
    }

    /// A key bound to a Lua function: send it to `client.key` with the keys before it.
    fn forward_key(&self, keystroke: &gpui::Keystroke, binding: &[String], repeat: bool) {
        let model = self.model();
        let (Some(session), Some(block)) = (model.selected.clone(), self.focused_block()) else {
            return;
        };
        let Some(key) = keys::key_event(keystroke, repeat) else {
            return;
        };
        let mut payload = json!({"session_id": session, "block_id": block, "key": key});
        if binding.len() > 1 {
            payload["prefix"] = json!(binding[..binding.len() - 1]);
        }
        self.host().handle.send(HostCommand::Input {
            session_id: session,
            block_id: block,
            method: "client.key",
            payload,
            claim: true,
        });
    }

    /// The app, system, server and keymap state, for bug reports.
    pub(super) fn debug_report(&self) -> String {
        let mut report = format!(
            "rexia {}\nOS: {} {}\nWayland display: {}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "none".into()),
        );
        for host in &self.hosts {
            let m = host.handle.snapshot();
            report.push_str(&format!(
                "Host {} ({}): {:?}, API revision {}, {} sessions, client {}\n",
                host.config.label,
                host.handle.endpoint,
                m.connection,
                m.api_revision,
                m.sessions.len(),
                m.client_id.unwrap_or_default(),
            ));
        }
        if let Some(path) = &self.keymap.config_path {
            report.push_str(&format!("Config: {path}\n"));
        }
        for line in &self.keymap.diagnostics {
            report.push_str(&format!("Keymap: {line}\n"));
        }
        report
    }

    /// Answer other clients' `client.perform` requests.
    pub(super) fn handle_performs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let requests: Vec<PerformRequest> = std::mem::take(
            &mut self
                .host()
                .handle
                .model
                .lock()
                .expect("host model")
                .performs,
        );
        for request in requests {
            let target = Target {
                session_id: request.session_id.clone(),
                block_id: request.block_id.clone(),
            };
            let args = if request.args.is_null() {
                json!({})
            } else {
                request.args.clone()
            };
            let result = if actions::find(&request.action).is_some() {
                self.run_action(&request.action, args, &request.origin, target, window, cx)
            } else {
                Err((
                    "not_supported".into(),
                    format!("rexia has no action {}", request.action),
                ))
            };
            self.host().handle.send(HostCommand::PerformReply {
                call_id: request.call_id,
                result,
            });
        }
    }

    /// Run an action by name: rexia's own here, the server's through `client.action`.
    pub fn run_action(
        &mut self,
        name: &str,
        args: Value,
        origin: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Outcome {
        if actions::find(name).is_some() {
            return self.perform(name, &args, target, window, cx);
        }
        let model = self.model();
        let session = target.session_id.or(model.selected.clone());
        let mut payload = json!({"action": name, "origin": origin});
        if !args.is_null() && args.as_object().is_some_and(|a| !a.is_empty()) {
            payload["args"] = args;
        }
        if let Some(session) = session {
            payload["session_id"] = json!(session);
            if let Some(block) = target.block_id {
                payload["block_id"] = json!(block);
            }
        }
        self.host().handle.call("client.action", payload);
        Ok(Value::Null)
    }

    fn perform(
        &mut self,
        name: &str,
        args: &Value,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let model = self.model();
        let session = target.session_id.clone().or(model.selected.clone());
        let view = session.as_ref().and_then(|s| model.views.get(s)).cloned();
        let window_view = view.as_ref().and_then(View::active_window).cloned();
        let block = target.block_id.clone().or_else(|| {
            window_view
                .as_ref()
                .and_then(|w| w.focused_block_id.clone())
        });
        let need_session = || session.clone().ok_or_else(|| refused("No session is open"));
        let need_block = || block.clone().ok_or_else(|| refused("No pane is focused"));
        let call =
            |ws: &Workspace, method: &str, payload: Value| ws.host().handle.call(method, payload);
        let direction = arg(args, "direction").map(str::to_owned);

        match name {
            "client.close" => {
                let block = need_block()?;
                let session = need_session()?;
                let panes = window_view.as_ref().map_or(0, |w| w.panes().len());
                if panes > 1 {
                    call(
                        self,
                        "block.close",
                        json!({"session_id": session, "block_id": block}),
                    );
                } else {
                    let window_id = window_view.as_ref().map(|w| w.id().to_owned());
                    call(
                        self,
                        "session.close_window",
                        json!({"session_id": session, "window_id": window_id}),
                    );
                }
            }
            "client.window.fullscreen.toggle" => window.toggle_fullscreen(),
            "client.version.copy" => {
                let host = self.host().handle.clone();
                cx.spawn(async move |_, cx| {
                    let server = host.request("server.version", json!({})).await.ok();
                    let text = format!(
                        "rexia {}\nRex server {} (API revision {})",
                        env!("CARGO_PKG_VERSION"),
                        server
                            .as_ref()
                            .and_then(|v| v["rex_version"].as_str())
                            .unwrap_or("unknown"),
                        server
                            .as_ref()
                            .and_then(|v| v["api_revision"].as_u64())
                            .unwrap_or(0),
                    );
                    let _ = cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
                })
                .detach();
            }
            "client.debug.copy" => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.debug_report()))
            }
            "client.diagnostics" => {
                let report = self.debug_report();
                self.open_diagnostics(report, cx);
            }
            "client.palette.toggle" => self.open_list(overlay::ListKind::Commands, window, cx),
            "session.switch" => self.open_list(overlay::ListKind::Sessions, window, cx),
            "pane.go_to" => self.open_list(overlay::ListKind::GoTo, window, cx),
            "pane.view_activity" => self.open_list(overlay::ListKind::Activity, window, cx),
            "pane.move_to_tab" => self.open_list(overlay::ListKind::MoveToTab, window, cx),
            "pane.go_to_directory" => {
                self.open_list(overlay::ListKind::Directory { insert: false }, window, cx)
            }
            "pane.insert_path" => {
                self.open_list(overlay::ListKind::Directory { insert: true }, window, cx)
            }
            "client.host.add" => self.open_add_host(window, cx),
            "client.host.remove" => self.open_list(overlay::ListKind::RemoveHost, window, cx),
            "client.find.open" => self.open_find(window, cx),
            "client.find.next" => self.find_step(true, window, cx),
            "client.find.previous" => self.find_step(false, window, cx),
            "session.rename" => {
                let id = need_session()?;
                let label = model
                    .session(&id)
                    .map(|s| s.label.clone())
                    .unwrap_or_default();
                self.open_rename(overlay::RenameTarget::Session(id), label, window, cx);
            }
            "client.tab.rename" => {
                let window_view = window_view
                    .as_ref()
                    .ok_or_else(|| refused("No tab is open"))?;
                let label = titles::tab_title(window_view, &model);
                self.open_rename(
                    overlay::RenameTarget::Window(window_view.id().to_owned()),
                    label,
                    window,
                    cx,
                );
            }
            "pane.rename" => {
                let block = need_block()?;
                let label = window_view
                    .as_ref()
                    .and_then(|w| w.block(&block))
                    .map(|b| titles::pane_title(b, model.blocks.get(&block)))
                    .unwrap_or_default();
                self.open_rename(overlay::RenameTarget::Block(block), label, window, cx);
            }
            "session.close" => {
                let id = need_session()?;
                let label = model
                    .session(&id)
                    .map(|s| s.label.clone())
                    .unwrap_or_default();
                let tabs = view.as_ref().map_or(0, |v| v.windows.len());
                let panes = view.as_ref().map_or(0, |v| {
                    v.windows.iter().map(|w| w.blocks().count()).sum::<usize>()
                });
                self.open_confirm(
                    format!("Close {label}?"),
                    format!("The programs running in its {panes} panes across {tabs} tabs end."),
                    "Close Session".into(),
                    overlay::Confirm::CloseSession { id },
                    cx,
                );
            }
            "client.notice.dismiss" => self.notice = None,
            "client.config.copy_path" => {
                let path = self
                    .keymap
                    .config_path
                    .clone()
                    .ok_or_else(|| refused("The server didn't report its config file"))?;
                cx.write_to_clipboard(ClipboardItem::new_string(path));
            }
            "client.config.reload" => {
                if !self.host().handle.endpoint.is_local() {
                    return Err(refused(
                        "Only the local server's config can be reloaded from here",
                    ));
                }
                call(self, "server.config_reload", json!({}));
            }
            "client.mode.enter" => {
                let mode =
                    arg(args, "name").ok_or_else(|| refused("Enter Mode needs a mode name"))?;
                if !self.keymap.modes.contains_key(mode) {
                    return Err(refused(format!("There's no mode named {mode}")));
                }
                self.modes.push(mode.to_owned());
            }
            "client.mode.exit" => {
                self.modes.pop();
            }
            "client.mode.exit_all" => self.modes.clear(),

            "session.new" => self.new_session(self.active, cx),
            "session.select" => {
                let id = arg(args, "session_id")
                    .ok_or_else(|| refused("Go to Session needs a session_id"))?;
                if !model.sessions.iter().any(|s| s.session_id == id) {
                    return Err(refused(format!("There's no session {id} on this host")));
                }
                self.select_session(self.active, id.to_owned(), cx);
            }
            "session.next" | "session.previous" => {
                let ids: Vec<_> = model
                    .sessions
                    .iter()
                    .map(|s| s.session_id.clone())
                    .collect();
                if ids.is_empty() {
                    return Err(refused("There are no sessions"));
                }
                let current = model
                    .selected
                    .as_ref()
                    .and_then(|s| ids.iter().position(|i| i == s))
                    .unwrap_or(0);
                let next = if name == "session.next" {
                    (current + 1) % ids.len()
                } else {
                    (current + ids.len() - 1) % ids.len()
                };
                self.select_session(self.active, ids[next].clone(), cx);
            }
            "session.refresh" => self.host().handle.send(HostCommand::Reconnect),
            "session.collapse" => {
                if let Some(id) = session {
                    self.expanded.remove(&id);
                }
            }
            "session.expand" => {
                if let Some(id) = session {
                    self.expanded.insert(id);
                }
            }
            "session.collapse_others" => {
                self.expanded.retain(|id| Some(id) == session.as_ref());
            }
            "client.host.switch" => {
                let next = match arg(args, "host") {
                    Some(wanted) => self
                        .hosts
                        .iter()
                        .position(|h| h.config.id == wanted || h.config.label == wanted)
                        .ok_or_else(|| refused(format!("There's no host {wanted}")))?,
                    None => {
                        self.open_list(overlay::ListKind::Hosts, window, cx);
                        return Ok(Value::Null);
                    }
                };
                self.switch_host(next, cx);
            }

            "client.tab.new" => self.new_tab(cx),
            "client.tab.close" => {
                let id = window_view
                    .as_ref()
                    .map(|w| w.id().to_owned())
                    .ok_or_else(|| refused("No tab is open"))?;
                self.close_tab(id, cx);
            }
            "client.tab.close_others" => {
                let view = view.as_ref().ok_or_else(|| refused("No session is open"))?;
                let session = need_session()?;
                let keep = view.active_window_id.clone();
                for window in view.windows.iter().filter(|w| w.window_id != keep) {
                    call(
                        self,
                        "session.close_window",
                        json!({"session_id": session, "window_id": window.id()}),
                    );
                }
            }
            "client.tab.next" | "client.tab.previous" => {
                let method = if name == "client.tab.next" {
                    "session.focus_next_window"
                } else {
                    "session.focus_previous_window"
                };
                call(
                    self,
                    method,
                    json!({"session_id": need_session()?, "wrap": true}),
                );
            }
            "client.tab.goto" => {
                let index = args
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| refused("Go to Tab needs an index"))?;
                let view = view.as_ref().ok_or_else(|| refused("No session is open"))?;
                let window = view
                    .windows
                    .get(index.saturating_sub(1) as usize)
                    .or(if index >= 9 {
                        view.windows.last()
                    } else {
                        None
                    })
                    .ok_or_else(|| refused(format!("There's no tab {index}")))?;
                self.select_tab(window.id().to_owned(), cx);
            }
            "client.tab.move" | "client.tab.move.backward" | "client.tab.move.forward" => {
                let view = view.as_ref().ok_or_else(|| refused("No session is open"))?;
                let current = view
                    .windows
                    .iter()
                    .position(|w| w.window_id == view.active_window_id)
                    .ok_or_else(|| refused("No tab is open"))?;
                let target_index = match name {
                    "client.tab.move.backward" => current.checked_sub(1),
                    "client.tab.move.forward" => {
                        Some(current + 1).filter(|i| *i < view.windows.len())
                    }
                    _ => args
                        .get("index")
                        .and_then(Value::as_u64)
                        .map(|i| (i as usize).saturating_sub(1).min(view.windows.len() - 1)),
                };
                if let Some(to) = target_index.filter(|to| *to != current) {
                    // Placing before the tab now at the target, or at the end when moving past it.
                    let before = if to > current {
                        view.windows.get(to + 1)
                    } else {
                        view.windows.get(to)
                    };
                    let mut payload = json!({"session_id": need_session()?, "window_id": view.windows[current].id()});
                    if let Some(before) = before {
                        payload["before_window_id"] = json!(before.id());
                    }
                    call(self, "session.move_window", payload);
                }
            }

            "pane.split" | "pane.split.left" | "pane.split.right" | "pane.split.up"
            | "pane.split.down" | "pane.split.auto" => {
                let wanted = match name {
                    "pane.split" => direction.clone().unwrap_or_else(|| "auto".into()),
                    other => other.trim_start_matches("pane.split.").to_owned(),
                };
                let wanted = if wanted == "auto" {
                    let rect = block.as_ref().and_then(|b| {
                        window_view
                            .as_ref()?
                            .panes()
                            .into_iter()
                            .find(|p| &p.block_id == b)
                            .map(|p| p.rect)
                    });
                    let area = self.area.size;
                    match rect {
                        // Split the longer side, measured in pixels.
                        Some(r)
                            if f64::from(f32::from(area.width)) * r.w
                                >= f64::from(f32::from(area.height)) * r.h * 1.6 =>
                        {
                            "right".to_owned()
                        }
                        Some(_) => "down".to_owned(),
                        None => "right".to_owned(),
                    }
                } else {
                    wanted
                };
                let horizontal = matches!(wanted.as_str(), "left" | "right");
                let after = matches!(wanted.as_str(), "right" | "down");
                self.split(block.clone(), horizontal, after);
            }
            "pane.focus" | "pane.focus.left" | "pane.focus.right" | "pane.focus.up"
            | "pane.focus.down" => {
                let dir = direction
                    .or_else(|| name.strip_prefix("pane.focus.").map(str::to_owned))
                    .ok_or_else(|| refused("Focus Pane needs a direction"))?;
                call(
                    self,
                    "session.focus_direction",
                    json!({"session_id": need_session()?, "direction": dir}),
                );
            }
            "pane.focus_next" | "pane.focus_previous" => {
                let method = if name == "pane.focus_next" {
                    "session.focus_next_pane"
                } else {
                    "session.focus_previous_pane"
                };
                call(
                    self,
                    method,
                    json!({"session_id": need_session()?, "wrap": true}),
                );
            }
            "pane.move" | "pane.move.left" | "pane.move.right" | "pane.move.up"
            | "pane.move.down" => {
                let dir = direction
                    .or_else(|| name.strip_prefix("pane.move.").map(str::to_owned))
                    .ok_or_else(|| refused("Move Pane needs a direction"))?;
                let block = need_block()?;
                let panes = window_view
                    .as_ref()
                    .map(view::Window::panes)
                    .unwrap_or_default();
                let other = view::neighbor(&panes, &block, &dir)
                    .ok_or_else(|| refused(format!("There's no pane to the {dir}")))?;
                call(
                    self,
                    "session.swap_blocks",
                    json!({"session_id": need_session()?, "block_id": block, "other_block_id": other.block_id}),
                );
            }
            "pane.float" => self.toggle_float(need_block()?),
            "pane.new_floating" => call(
                self,
                "session.new_layer",
                json!({"session_id": need_session()?, "bounds": [0.15, 0.12, 0.7, 0.72], "layout": shell_layout(), "focus": true}),
            ),
            "pane.minimize_floating" | "pane.minimize_all_floating" => {
                let window = window_view
                    .as_ref()
                    .ok_or_else(|| refused("No tab is open"))?;
                let session = need_session()?;
                let floating: Vec<&view::Layer> =
                    window.layers.iter().filter(|l| l.kind != "tiled").collect();
                if floating.is_empty() {
                    return Err(refused("This tab has no floating panes"));
                }
                let focused_layer = block
                    .as_ref()
                    .and_then(|b| window.layer_of(b))
                    .filter(|l| l.kind != "tiled");
                let layers: Vec<&view::Layer> = if name == "pane.minimize_floating" {
                    focused_layer
                        .into_iter()
                        .chain(floating.last().copied())
                        .take(1)
                        .collect()
                } else {
                    floating.clone()
                };
                // Minimize what's showing; when everything is minimized, bring it back.
                let showing = layers
                    .iter()
                    .any(|l| l.state.as_deref() != Some("minimized"));
                for layer in layers {
                    let state = if showing { "minimized" } else { "normal" };
                    call(
                        self,
                        "session.set_layer_state",
                        json!({"session_id": session, "layer_id": layer.layer_id, "state": state, "focus": !showing}),
                    );
                }
            }
            "pane.zoom" => self.toggle_zoom(need_block()?),
            "pane.close" => self.close_pane(need_block()?),
            "pane.close_others" => {
                let window = window_view
                    .as_ref()
                    .ok_or_else(|| refused("No tab is open"))?;
                let keep = need_block()?;
                let session = need_session()?;
                for other in window
                    .blocks()
                    .filter_map(|b| b.block_id.clone())
                    .filter(|b| *b != keep)
                {
                    call(
                        self,
                        "block.close",
                        json!({"session_id": session, "block_id": other}),
                    );
                }
            }
            "pane.balance" => {
                let window = window_view
                    .as_ref()
                    .ok_or_else(|| refused("No tab is open"))?;
                let session = need_session()?;
                for ratio in view::balanced(window) {
                    call(
                        self,
                        "session.resize_pane",
                        json!({"session_id": session,
                        "direction": if ratio.horizontal { "horizontal" } else { "vertical" },
                        "before_block_id": ratio.before_block, "after_block_id": ratio.after_block, "ratio": ratio.ratio}),
                    );
                }
            }
            "pane.resize" => {
                let dir = direction.ok_or_else(|| refused("Resize Pane needs a direction"))?;
                let amount = args
                    .get("amount")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.05)
                    .abs();
                let window = window_view
                    .as_ref()
                    .ok_or_else(|| refused("No tab is open"))?;
                let divider = view::nudge(window, &need_block()?, &dir, amount)
                    .ok_or_else(|| refused(format!("There's no divider to move {dir}")))?;
                call(
                    self,
                    "session.resize_pane",
                    json!({"session_id": need_session()?,
                    "direction": if divider.horizontal { "horizontal" } else { "vertical" },
                    "before_block_id": divider.before_block, "after_block_id": divider.after_block, "ratio": divider.ratio}),
                );
            }
            "pane.move_to_new_tab" => {
                let block = need_block()?;
                let session = need_session()?;
                let host = self.host().handle.clone();
                // A new tab starts with a shell; the pane moves in beside it and the shell closes.
                cx.spawn(async move |_, _| {
                    let Ok(created) = host
                        .request("session.new_window", json!({"session_id": session, "layout": shell_layout(), "focus": true}))
                        .await
                    else {
                        return;
                    };
                    let Some(placeholder) = created["block_ids"][0].as_str().map(str::to_owned) else { return };
                    if host
                        .request("session.move_block", json!({"session_id": session, "block_id": block, "anchor_block_id": placeholder}))
                        .await
                        .is_ok()
                    {
                        let _ = host.request("block.close", json!({"session_id": session, "block_id": placeholder})).await;
                        let _ = host.request("session.focus_block", json!({"session_id": session, "block_id": block})).await;
                    }
                })
                .detach();
            }
            "pane.copy_id" => cx.write_to_clipboard(ClipboardItem::new_string(need_block()?)),
            "pane.paste"
            | "pane.copy"
            | "pane.select_all"
            | "client.prompt.previous"
            | "client.prompt.next"
            | "client.scroll.page_up"
            | "client.scroll.page_down" => {
                let block = need_block()?;
                let pane = self
                    .pane_entity(&block)
                    .ok_or_else(|| refused("That pane isn't shown"))?;
                pane.update(cx, |pane, cx| match name {
                    "pane.paste" => pane.paste(cx),
                    "pane.copy" => pane.copy(cx),
                    "pane.select_all" => pane.select_all(),
                    "client.prompt.previous" => pane.jump_to_prompt(-1),
                    "client.prompt.next" => pane.jump_to_prompt(1),
                    "client.scroll.page_up" => pane.scroll_pages(-1),
                    _ => pane.scroll_pages(1),
                });
            }
            "pane.send_key" => {
                let spelling = arg(args, "key").ok_or_else(|| refused("Send Key needs a key"))?;
                let key = keys::event_from_spelling(spelling)
                    .ok_or_else(|| refused(format!("rexia can't send {spelling}")))?;
                let (session, block) = (need_session()?, need_block()?);
                self.host().handle.send(HostCommand::Input {
                    session_id: session.clone(),
                    block_id: block.clone(),
                    method: "block.event",
                    payload: json!({"session_id": session, "block_id": block, "event": {"type": "key", "payload": key}}),
                    claim: true,
                });
            }

            "client.font.increase" | "client.font.decrease" | "client.font.reset" => {
                let size = match name {
                    "client.font.increase" => Some((self.font.size + 1.).min(72.)),
                    "client.font.decrease" => Some((self.font.size - 1.).max(6.)),
                    _ => None,
                };
                self.settings.terminal.font_size = size;
                self.font = super::terminal_font(&self.settings);
                let font = self.font.clone();
                for pane in self.panes.values() {
                    pane.update(cx, |pane, cx| pane.set_font(font.clone(), cx));
                }
                self.save_settings();
            }
            "client.sidebar.toggle" => self.toggle_sidebar(cx),
            other => {
                return Err((
                    "not_supported".into(),
                    format!("rexia can't perform {other} yet"),
                ));
            }
        }
        cx.notify();
        Ok(Value::Null)
    }
}
