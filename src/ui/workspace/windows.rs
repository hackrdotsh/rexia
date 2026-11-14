//! Several workspace windows in one rexia: New Window, Close Window, and Move Tab to New
//! Window. Each window is its own Rex client. The server keeps one active tab per session,
//! so windows that split a session's tabs between them each pin the tab they show.
use super::Workspace;
use crate::{rex::host::HostCommand, settings::Settings};
use gpui::{AnyWindowHandle, Context, Window};
use serde_json::json;

impl Workspace {
    /// Whether this client pins its own tab of `session` on host `host`.
    pub fn pinned(&self, host: usize, session: &str) -> bool {
        self.hosts.get(host).is_some_and(|h| {
            h.handle
                .model
                .lock()
                .expect("host model")
                .pins
                .contains_key(session)
        })
    }

    /// Show a tab: make it the session's active tab, and this window's own if it pins one.
    pub fn focus_tab(&mut self, host: usize, session: String, window: String) {
        let Some(entry) = self.hosts.get(host) else {
            return;
        };
        if self.pinned(host, &session) {
            entry.handle.send(HostCommand::Pin {
                session: session.clone(),
                window: Some(window.clone()),
            });
        }
        entry.handle.call(
            "session.focus_window",
            json!({"session_id": session, "window_id": window}),
        );
    }

    /// The tab after (or before) the shown one, for windows that pin their own tab.
    pub(super) fn neighbour_tab(&self, forward: bool) -> Option<String> {
        let model = self.model();
        let view = model.view()?;
        let count = view.windows.len();
        let current = view
            .windows
            .iter()
            .position(|w| w.window_id == view.active_window_id)?;
        let next = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        Some(view.windows[next].id().to_owned())
    }

    /// Settings for another window: the same hosts and preferences, showing `session` on
    /// this window's host.
    fn settings_for_window(&self, session: Option<String>) -> Settings {
        let mut settings = self.settings.clone();
        settings.last_host = Some(self.host().config.id.clone());
        settings.last_session = session;
        settings
    }

    /// `client.window.new`: another window, with a new session on this window's host.
    pub fn open_new_window(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings_for_window(None);
        let path = self.settings_path.clone();
        cx.defer(move |cx| {
            let opened = crate::app::open_workspace(settings, path, cx, |ws, _, cx| {
                ws.new_session(ws.active, cx);
            });
            if let Err(error) = opened {
                eprintln!("rexia: couldn't open a window: {error:#}");
            }
        });
    }

    /// `client.window.close`: close this window. The last one closing quits rexia.
    pub fn close_window(&mut self, window: &mut Window) {
        window.remove_window();
    }

    /// `client.tab.move_to_new_window`: show the current tab in a window of its own, and
    /// another of the session's tabs here.
    pub fn move_tab_to_new_window(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let model = self.model();
        let session = model.selected.clone().ok_or("No session is open")?;
        let view = model.view().ok_or("No session is open")?;
        let tab = view
            .active_window()
            .map(|w| w.id().to_owned())
            .ok_or("No tab is open")?;
        if view.windows.len() < 2 {
            return Err("A session's only tab is already in its own window".into());
        }
        let stay = self.neighbour_tab(false).ok_or("No other tab")?;
        self.hosts[self.active].handle.send(HostCommand::Pin {
            session: session.clone(),
            window: Some(stay),
        });
        let settings = self.settings_for_window(Some(session.clone()));
        let path = self.settings_path.clone();
        cx.defer(move |cx| {
            let opened = crate::app::open_workspace(settings, path, cx, move |ws, _, _| {
                ws.hosts[ws.active].handle.send(HostCommand::Pin {
                    session,
                    window: Some(tab),
                });
            });
            if let Err(error) = opened {
                eprintln!("rexia: couldn't open a window: {error:#}");
            }
        });
        cx.notify();
        Ok(())
    }

    /// Share a preference change with rexia's other windows.
    pub(super) fn share_settings(&self, cx: &mut Context<Self>) {
        let shared = self.settings.clone();
        let me = self.window_handle;
        cx.defer(move |cx| {
            for handle in cx.windows() {
                if Some(handle) == me {
                    continue;
                }
                if let Some(other) = handle.downcast::<Workspace>() {
                    let _ = other.update(cx, |ws, _, cx| ws.adopt_settings(&shared, cx));
                }
            }
        });
    }

    /// Take another window's preferences; which host and session it shows stay its own.
    fn adopt_settings(&mut self, other: &Settings, cx: &mut Context<Self>) {
        self.settings.interface = other.interface.clone();
        self.settings.terminal = other.terminal.clone();
        self.settings.notifications = other.notifications.clone();
        self.settings.client_label = other.client_label.clone();
        self.apply_settings(cx);
        cx.notify();
    }

    /// The window this workspace fills.
    pub fn window_handle(&self) -> Option<AnyWindowHandle> {
        self.window_handle
    }
}
