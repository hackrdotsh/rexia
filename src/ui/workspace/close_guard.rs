//! Ask before closing busy work: closing a pane or tab (or the others beside one) whose
//! program is working, needs attention, failed, or finished without being seen asks first,
//! naming those programs. A program in the question goes to its pane when clicked; Enter
//! closes, Escape keeps them. Closing a session always asks, and names them too.
use super::{Workspace, overlay::Confirm};
use crate::{
    rex::{activity::ProgramState, view},
    ui::titles,
};
use gpui::Context;
use serde_json::json;

/// What's being closed.
#[derive(Clone, Debug, PartialEq)]
pub enum CloseTarget {
    Pane(String),
    Tab(String),
    /// Every pane in the tab but this one.
    OtherPanes(String),
    /// Every tab in the session but this one.
    OtherTabs(String),
}

/// A program that's busy in something about to close.
#[derive(Clone, Debug, PartialEq)]
pub struct Busy {
    pub block: String,
    pub title: String,
    /// What it's doing, as its badge says: Working, Needs input, Done…
    pub state: String,
}

impl Workspace {
    /// The busy programs among `blocks` of the session on screen.
    pub fn busy_in(&self, blocks: &[String]) -> Vec<Busy> {
        let model = self.model();
        let Some(view) = model.view() else {
            return Vec::new();
        };
        blocks
            .iter()
            .filter_map(|id| {
                let state = model.blocks.get(id);
                let record = state.and_then(|s| s.status());
                let unseen = self.tracker.is_unseen(&(self.active, id.clone()));
                let busy = match record.map(|r| r.state) {
                    Some(ProgramState::Working | ProgramState::Blocked | ProgramState::Error) => {
                        true
                    }
                    Some(ProgramState::Done) | Some(ProgramState::Idle) | None => unseen,
                };
                if !busy {
                    return None;
                }
                let block = view.windows.iter().find_map(|w| w.block(id))?;
                Some(Busy {
                    block: id.clone(),
                    title: titles::pane_title(block, state),
                    state: record.map_or("Unread", |r| r.badge()).to_owned(),
                })
            })
            .collect()
    }

    fn blocks_closing(&self, target: &CloseTarget) -> Vec<String> {
        let model = self.model();
        let Some(view) = model.view() else {
            return Vec::new();
        };
        let ids = |w: &view::Window| -> Vec<String> {
            w.blocks().filter_map(|b| b.block_id.clone()).collect()
        };
        match target {
            CloseTarget::Pane(block) => vec![block.clone()],
            CloseTarget::Tab(window) => view
                .windows
                .iter()
                .find(|w| w.id() == window)
                .map(ids)
                .unwrap_or_default(),
            CloseTarget::OtherPanes(keep) => view
                .windows
                .iter()
                .find(|w| w.block(keep).is_some())
                .map(ids)
                .unwrap_or_default()
                .into_iter()
                .filter(|b| b != keep)
                .collect(),
            CloseTarget::OtherTabs(keep) => view
                .windows
                .iter()
                .filter(|w| w.id() != keep)
                .flat_map(ids)
                .collect(),
        }
    }

    /// Close `target`, asking first when a program in it is busy.
    pub fn request_close(&mut self, target: CloseTarget, cx: &mut Context<Self>) {
        let busy = self.busy_in(&self.blocks_closing(&target));
        if busy.is_empty() {
            return self.close_now(target);
        }
        let (title, confirm) = match &target {
            CloseTarget::Pane(_) => ("Close this pane?", "Close Pane"),
            CloseTarget::Tab(_) => ("Close this tab?", "Close Tab"),
            CloseTarget::OtherPanes(_) => ("Close the other panes?", "Close Panes"),
            CloseTarget::OtherTabs(_) => ("Close the other tabs?", "Close Tabs"),
        };
        let body = if busy.len() == 1 {
            "This program is still busy, and closing ends it.".to_owned()
        } else {
            format!(
                "These {} programs are still busy, and closing ends them.",
                busy.len()
            )
        };
        self.open_confirm_busy(
            title.into(),
            body,
            confirm.into(),
            Confirm::Close(target),
            busy,
            cx,
        );
    }

    /// Close without asking.
    pub(super) fn close_now(&mut self, target: CloseTarget) {
        let Some(session) = self.session_id() else {
            return;
        };
        match &target {
            CloseTarget::Pane(block) => self.close_pane(block.clone()),
            CloseTarget::Tab(window) => self.call(
                "session.close_window",
                json!({"session_id": session, "window_id": window}),
            ),
            CloseTarget::OtherPanes(_) => {
                for block in self.blocks_closing(&target) {
                    self.close_pane(block);
                }
            }
            CloseTarget::OtherTabs(keep) => {
                let windows: Vec<String> = self
                    .model()
                    .view()
                    .map(|v| {
                        v.windows
                            .iter()
                            .map(|w| w.id().to_owned())
                            .filter(|w| w != keep)
                            .collect()
                    })
                    .unwrap_or_default();
                for window in windows {
                    self.call(
                        "session.close_window",
                        json!({"session_id": session, "window_id": window}),
                    );
                }
            }
        }
    }
}
