use super::Workspace;
use crate::rex::{
    data::TERMINAL_CREATOR,
    host::{HostModel, PaneStatus},
    view::View,
};
use gpui::{Context, Focusable, Window};
use std::time::{Duration, Instant};

// A slow or unavailable server must not leave the old tab on screen indefinitely.
const WAIT_FOR_FRAME: Duration = Duration::from_millis(800);

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Tab {
    host: String,
    session: String,
    window: String,
}

pub(super) struct Pending {
    target: Tab,
    since: Instant,
    _timeout: gpui::Task<()>,
}

impl Workspace {
    fn presented_in(&self, model: &HostModel) -> Option<&Tab> {
        self.presented_tab.as_ref().filter(|tab| {
            tab.host == self.host().config.id
                && model.selected.as_ref() == Some(&tab.session)
                && model
                    .view()
                    .is_some_and(|view| view.window(&tab.window).is_some())
        })
    }

    pub(super) fn apply_presented_tab(&self, model: &mut HostModel) {
        if let Some(tab) = self.presented_in(model)
            && let Some(view) = model.views.get_mut(&tab.session)
        {
            view.active_window_id = Some(tab.window.clone());
        }
    }

    /// Keep the last drawn tab until its replacement can be painted alongside its tab
    /// selection. The actual active tab has already changed on Rex so its streams can attach.
    pub(super) fn sync_presented_tab(
        &mut self,
        model: &HostModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = model.selected.as_ref() else {
            self.presented_tab = None;
            self.pending_tab = None;
            return;
        };
        let Some(tab) = model.view().and_then(View::active_window) else {
            self.presented_tab = None;
            self.pending_tab = None;
            return;
        };
        let target = Tab {
            host: self.host().config.id.clone(),
            session: session.clone(),
            window: tab.id().to_owned(),
        };
        let already_shown = self.presented_in(model);
        if already_shown.is_none_or(|shown| *shown == target) {
            self.presented_tab = Some(target);
            self.pending_tab = None;
            return;
        }
        let ready = tab.panes().into_iter().all(|pane| {
            if !tab
                .block(&pane.block_id)
                .is_some_and(|block| block.creator_name == TERMINAL_CREATOR)
            {
                return true;
            }
            self.panes.contains_key(&pane.block_id)
                && model.panes.get(&pane.block_id).is_some_and(|handle| {
                    let pane = handle.snapshot();
                    pane.frame.is_some()
                        || matches!(
                            pane.status,
                            PaneStatus::Reconnecting(_) | PaneStatus::Closed
                        )
                })
        });
        let expired = self.pending_tab.as_ref().is_some_and(|pending| {
            pending.target == target && pending.since.elapsed() >= WAIT_FOR_FRAME
        });
        if ready || expired {
            self.presented_tab = Some(target);
            self.pending_tab = None;
        } else if self
            .pending_tab
            .as_ref()
            .is_none_or(|pending| pending.target != target)
        {
            // Keep shortcut handling active without sending text to either terminal mid-switch.
            if self
                .focused
                .as_ref()
                .and_then(|id| self.panes.get(id))
                .is_some_and(|pane| pane.read(cx).focus_handle(cx).is_focused(window))
            {
                window.focus(&self.focus, cx);
                self.focused = None;
            }
            let since = Instant::now();
            let timeout = cx.background_executor().timer(WAIT_FOR_FRAME);
            self.pending_tab = Some(Pending {
                target,
                since,
                _timeout: cx.spawn_in(window, async move |workspace, cx| {
                    timeout.await;
                    let _ = workspace.update_in(cx, |ws, window, cx| ws.sync(window, cx));
                }),
            });
        }
    }
}
