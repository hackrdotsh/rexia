use super::{Workspace, preferences};
use crate::rex::{
    data::TERMINAL_CREATOR,
    host::{HostCommand, HostModel},
    view::View,
};
use crate::ui::theme::Theme;
use gpui::{Context, Focusable, Role, Window, div, prelude::*, px};
use serde_json::json;

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Tab {
    host: String,
    session: String,
    window: String,
}

pub(super) struct Pending {
    target: Tab,
    returning: bool,
}

impl Workspace {
    fn presented_for_session(&self, model: &HostModel) -> Option<&Tab> {
        self.presented_tab.as_ref().filter(|tab| {
            tab.host == self.host().config.id && model.selected.as_ref() == Some(&tab.session)
        })
    }

    fn presented_in(&self, model: &HostModel) -> Option<&Tab> {
        self.presented_for_session(model).filter(|tab| {
            model
                .view()
                .is_some_and(|view| view.window(&tab.window).is_some())
        })
    }

    /// Return whether to show the loading card. A redraw can see the close before
    /// sync records the pending switch, so compare the presented tab to this model.
    pub(super) fn apply_presented_tab(&self, model: &mut HostModel) -> bool {
        let shown = self.presented_in(model).map(|tab| tab.window.clone());
        let loading = self.presented_for_session(model).is_some()
            && model
                .view()
                .and_then(View::active_window)
                .is_some_and(|active| shown.as_deref() != Some(active.id()));
        if (shown.is_some() || loading)
            && let Some(view) = model
                .selected
                .as_ref()
                .and_then(|id| model.views.get_mut(id))
        {
            view.active_window_id = shown;
        }
        loading
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
        // A closed tab still counts as a previous presentation in this session.
        // Only the first view of a session may show its terminals as they attach.
        if self
            .presented_for_session(model)
            .is_none_or(|shown| *shown == target)
        {
            self.presented_tab = Some(target);
            self.pending_tab = None;
            return;
        }
        if self
            .pending_tab
            .as_ref()
            .is_some_and(|pending| pending.returning && pending.target == target)
        {
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
                && model
                    .panes
                    .get(&pane.block_id)
                    .is_some_and(|handle| handle.snapshot().frame.is_some())
        });
        if ready {
            self.presented_tab = Some(target);
            self.pending_tab = None;
        } else if self
            .pending_tab
            .as_ref()
            .is_none_or(|pending| pending.target != target)
        {
            // Keep shortcut handling active without sending text to either terminal mid-switch.
            if self.overlay.is_none()
                && (window.focused(cx).is_none()
                    || self
                        .focused
                        .as_ref()
                        .and_then(|id| self.panes.get(id))
                        .is_some_and(|pane| pane.read(cx).focus_handle(cx).is_focused(window)))
            {
                window.focus(&self.focus, cx);
                self.focused = None;
            }
            self.pending_tab = Some(Pending {
                target,
                returning: false,
            });
        }
    }

    pub(super) fn cancel_tab_switch(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(previous) = self.presented_in(&self.model()).cloned() else {
            return false;
        };
        let Some(pending) = self.pending_tab.as_mut() else {
            return false;
        };
        if pending.returning {
            return true;
        }
        // Hold the previous layout even if the last frame arrives before Rex confirms the return.
        pending.returning = true;
        let target = pending.target.clone();
        self.pending.clear();
        let host = self.host().handle.clone();
        if self.pinned(self.active, &previous.session) {
            host.send(HostCommand::Pin {
                session: previous.session.clone(),
                window: Some(previous.window.clone()),
            });
        }
        cx.spawn(async move |workspace, cx| {
            let result = host
                .request(
                    "session.focus_window",
                    json!({
                        "session_id": previous.session, "window_id": previous.window,
                    }),
                )
                .await;
            if let Err(reason) = result {
                let _ = workspace.update(cx, |ws, cx| {
                    if let Some(pending) = ws.pending_tab.as_mut()
                        && pending.target == target
                        && pending.returning
                    {
                        pending.returning = false;
                        ws.notice = Some(format!("Couldn't cancel the tab switch: {reason}"));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        cx.notify();
        true
    }

    pub(super) fn render_tab_wait(
        &self,
        can_cancel: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let returning = self
            .pending_tab
            .as_ref()
            .is_some_and(|pending| pending.returning);
        div()
            .id("switching-tab")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(theme.radius_small())
                    .bg(theme.surface)
                    .border(theme.border_width)
                    .border_color(theme.border)
                    .text_color(theme.text)
                    .text_size(px(13.))
                    .child(if returning {
                        "Returning to tab…"
                    } else {
                        "Connecting tab…"
                    })
                    .when(can_cancel && !returning, |card| {
                        card.child(
                            preferences::button("cancel-tab-switch", "Cancel (Esc)", false, theme)
                                .role(Role::Button)
                                .on_click(cx.listener(|ws, _, _, cx| {
                                    ws.cancel_tab_switch(cx);
                                })),
                        )
                    }),
            )
    }
}
