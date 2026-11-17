//! Sessions in the sidebar: rename, move and close them from a row's right-click menu,
//! close them from the button under the pointer, and drag rows to reorder them
//! (`session.move`; every client sees the order).
use super::{
    Workspace,
    actions::Target,
    context_menu::{Command, ContextMenu, Item},
    overlay::RenameTarget,
};
use gpui::{Context, Pixels, Point, Window};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionCommand {
    Rename,
    MoveUp,
    MoveDown,
    Close,
}

impl Workspace {
    /// The sessions of `host`, in the server's order.
    pub fn session_order(&self, host: usize) -> Vec<String> {
        self.hosts
            .get(host)
            .map(|h| {
                h.handle
                    .snapshot()
                    .sessions
                    .iter()
                    .map(|s| s.session_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Right-click on a session row.
    pub fn open_session_menu(
        &mut self,
        host: usize,
        session: String,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let order = self.session_order(host);
        let place = order.iter().position(|s| *s == session);
        let item = |label: &str, command: SessionCommand, enabled: bool| Item {
            label: label.to_owned(),
            command: Command::Session(command, host, session.clone()),
            enabled,
            shortcut: None,
        };
        let groups = vec![
            vec![item("Rename Session…", SessionCommand::Rename, true)],
            vec![
                item(
                    "Move Up",
                    SessionCommand::MoveUp,
                    place.is_some_and(|p| p > 0),
                ),
                item(
                    "Move Down",
                    SessionCommand::MoveDown,
                    place.is_some_and(|p| p + 1 < order.len()),
                ),
            ],
            vec![item("Close Session…", SessionCommand::Close, true)],
        ];
        self.context_menu = Some(Box::new(ContextMenu::new(session.clone(), at, groups)));
        cx.notify();
    }

    pub(super) fn session_command(
        &mut self,
        command: SessionCommand,
        host: usize,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            SessionCommand::Rename => self.rename_session(host, id, window, cx),
            SessionCommand::MoveUp => self.move_session(host, &id, -1),
            SessionCommand::MoveDown => self.move_session(host, &id, 1),
            SessionCommand::Close => self.close_session(host, id, window, cx),
        }
    }

    /// Rename a session, on whichever host it's on.
    pub fn rename_session(
        &mut self,
        host: usize,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if host != self.active {
            self.switch_host(host, cx);
        }
        let label = self
            .model()
            .session(&id)
            .map(|s| s.label.clone())
            .unwrap_or_default();
        self.open_rename(RenameTarget::Session(id), label, window, cx);
    }

    /// Close a session: Close Session asks first, naming busy programs.
    pub fn close_session(
        &mut self,
        host: usize,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if host != self.active {
            self.switch_host(host, cx);
        }
        let target = Target {
            session_id: Some(id),
            block_id: None,
        };
        if let Err((_, why)) =
            self.run_action("session.close", Value::Null, "sidebar", target, window, cx)
        {
            self.notice = Some(why);
        }
    }

    /// Move a session `by` places in its host's order.
    pub fn move_session(&mut self, host: usize, id: &str, by: isize) {
        let order = self.session_order(host);
        let Some(at) = order.iter().position(|s| s == id) else {
            return;
        };
        let to = at
            .saturating_add_signed(by)
            .min(order.len().saturating_sub(1));
        if to == at {
            return;
        }
        // Moving down lands before the session after the one it passes.
        let before = if to > at {
            order.get(to + 1)
        } else {
            order.get(to)
        };
        self.place_session(host, id, before.cloned());
    }

    /// Put a session just before `before`, or last.
    pub(super) fn place_session(&mut self, host: usize, id: &str, before: Option<String>) {
        if before.as_deref() == Some(id) {
            return;
        }
        let mut payload = json!({"session_id": id});
        if let Some(before) = before {
            payload["before_session_id"] = json!(before);
        }
        if let Some(entry) = self.hosts.get(host) {
            entry.handle.call("session.move", payload);
        }
    }
}
