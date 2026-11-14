//! The app-level actions: Restart Server, Change Shortcut, and Toggle Visibility.
use super::{
    Workspace,
    overlay::{Confirm, Dialog, ListKind, Overlay},
};
use crate::{
    keymap,
    rex::{endpoint::Endpoint, host::HostCommand},
    ui::{actions, keys},
};
use gpui::{Context, Keystroke, Window};
use serde_json::{Value, json};
use std::time::Duration;

impl Workspace {
    /// `client.server.restart`: ask before restarting this computer's server.
    pub fn ask_restart_server(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if !self.host().handle.endpoint.is_local() {
            return Err("Only this computer's Rex server can be restarted from here".into());
        }
        self.open_confirm(
            "Restart the Rex server?".into(),
            "Its sessions are saved and come back, but every program running in them ends.".into(),
            "Restart Server".into(),
            Confirm::RestartServer,
            cx,
        );
        Ok(())
    }

    /// Stop the server, wait for it to let go of its socket, then start it again with
    /// `rex server start` (`$REXIA_REX` names another rex). rexia reconnects by itself.
    pub(super) fn restart_server(&mut self, cx: &mut Context<Self>) {
        let endpoint = self.host().handle.endpoint.clone();
        self.host().handle.call("server.stop", json!({}));
        let rex = std::env::var("REXIA_REX").unwrap_or_else(|_| "rex".into());
        let args = self.restart_args();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    if let Endpoint::Unix(path) = &endpoint {
                        for _ in 0..100 {
                            if std::os::unix::net::UnixStream::connect(path).is_err() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                    std::process::Command::new(&rex)
                        .args(&args)
                        .output()
                        .map_err(|error| format!("Couldn't run {rex}: {error}"))
                })
                .await;
            let problem = match outcome {
                Ok(output) if output.status.success() => None,
                Ok(output) => {
                    let said = String::from_utf8_lossy(&output.stderr);
                    let line = said.lines().rev().find(|l| !l.trim().is_empty());
                    Some(format!(
                        "The server didn't start again: {}",
                        line.unwrap_or("rex server start failed")
                    ))
                }
                Err(message) => Some(message),
            };
            let _ = this.update(cx, |ws, cx| {
                ws.notice = problem;
                cx.notify();
            });
        })
        .detach();
    }

    /// `client.visibility.toggle`: bring rexia to the front, or hide it when it's in front.
    /// Compositors bind it with `rex do client.visibility.toggle`.
    pub fn toggle_visibility(&mut self, window: &mut Window) -> Value {
        if window.is_window_active() {
            (self.hide_window)(window);
            json!({"shown": false})
        } else {
            window.activate_window();
            json!({"shown": true})
        }
    }

    /// Hide the window some other way than minimizing it, such as in tests.
    pub fn set_window_hider(&mut self, hide: impl Fn(&mut Window) + 'static) {
        self.hide_window = std::rc::Rc::new(hide);
    }

    /// `client.shortcuts.edit`: choose an action to give a new key.
    pub fn edit_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_list(ListKind::Shortcuts, window, cx);
    }

    /// Wait for the next key, which becomes `action`'s shortcut.
    pub(super) fn record_shortcut(
        &mut self,
        action: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Some(Overlay::Dialog(Dialog::Shortcut { action, title }));
        // Keys come to the workspace, not a field or the terminal.
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The action Change Shortcut is waiting for a key for.
    pub fn recording_shortcut(&self) -> Option<String> {
        match &self.overlay {
            Some(Overlay::Dialog(Dialog::Shortcut { action, .. })) => Some(action.clone()),
            _ => None,
        }
    }

    /// The key pressed while recording: Esc keeps the old shortcut, Backspace removes it,
    /// anything else becomes it.
    pub(super) fn shortcut_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.recording_shortcut() else {
            return;
        };
        let m = &keystroke.modifiers;
        let plain = !m.control && !m.alt && !m.platform && !m.shift;
        let keys = match keystroke.key.as_str() {
            "escape" if plain => {
                self.close_overlay(window, cx);
                return;
            }
            "backspace" if plain => Vec::new(),
            _ => match keys::stroke(keystroke).spellings().into_iter().next() {
                Some(spelling) => vec![spelling],
                None => return,
            },
        };
        self.update_settings(|s| drop(s.shortcuts.insert(action, keys)), cx);
        let request = actions::keymap_request(&self.settings.shortcuts);
        for host in &self.hosts {
            host.handle.send(HostCommand::Keymap(request.clone()));
        }
        self.close_overlay(window, cx);
    }

    /// The keys bound to an action in the keymap in use, as Rex spells them.
    pub fn keys_for_action(&self, action: &str) -> Option<String> {
        self.keymap
            .keys_for(action, None)
            .map(|keys| keys.join(" "))
    }

    /// The keys an action has now, for the shortcut dialog.
    pub(super) fn shortcut_of(&self, action: &str) -> String {
        self.keymap
            .keys_for(action, None)
            .map(keymap::display)
            .unwrap_or_else(|| "None".into())
    }
}
