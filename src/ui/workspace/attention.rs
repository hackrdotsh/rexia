//! Program activity in the workspace: badges on tabs, sessions and pane headers, desktop
//! notifications and sounds for panes out of view, and the sidebar's activity list.
use super::{Workspace, activity_mark};
use crate::{
    notify::{Note, Notifier, Urgency},
    rex::{
        activity::ProgramState,
        host::{BlockState, HostModel},
        view::Window as ViewWindow,
    },
    sound::{self, Cue},
    ui::{
        components::{heading, small_icon_button, title},
        icons::icon,
        theme::Theme,
        titles,
    },
};
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px};

/// A tab mark: the state shown and its badge word.
type Mark = (ProgramState, &'static str);

/// A notification's key: the pane it's about, so a click can show it.
fn note_key(host: usize, session: &str, block: &str) -> String {
    format!("{host}\u{1f}{session}\u{1f}{block}")
}

fn parse_key(key: &str) -> Option<(usize, String, String)> {
    let mut parts = key.split('\u{1f}');
    let host = parts.next()?.parse().ok()?;
    Some((host, parts.next()?.to_owned(), parts.next()?.to_owned()))
}

/// One row of the activity list: a tab (or a pane, with Show each process) that reports.
struct Row {
    host: usize,
    session: String,
    session_label: String,
    block: String,
    title: String,
    detail: Option<String>,
    icon: &'static str,
    mark: Option<(ProgramState, &'static str)>,
    state: ProgramState,
}

impl Workspace {
    /// Use `notifier` for desktop notifications; clicking one shows its pane.
    pub fn set_notifier(
        &mut self,
        notifier: Notifier,
        clicks: flume::Receiver<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notifier = Some(notifier);
        self._tasks.push(cx.spawn_in(window, async move |this, cx| {
            while let Ok(key) = clicks.recv_async().await {
                if let Some(host) = super::tailscale_notes::parse_key(&key) {
                    let shown = this.update_in(cx, |ws, window, cx| {
                        ws.tailscale_note_clicked(host, window, cx)
                    });
                    if shown.is_err() {
                        break;
                    }
                    continue;
                }
                let Some((host, session, block)) = parse_key(&key) else {
                    continue;
                };
                let shown = this.update_in(cx, |ws, window, cx| {
                    if host < ws.hosts.len() {
                        ws.select_session(host, session.clone(), cx);
                        ws.hosts[host].handle.call(
                            "session.focus_block",
                            serde_json::json!({"session_id": session, "block_id": block}),
                        );
                        window.activate_window();
                    }
                });
                if shown.is_err() {
                    break;
                }
            }
        }));
    }

    /// How many panes hold attention the user hasn't seen.
    pub fn unseen_attention(&self) -> usize {
        self.tracker.unseen_count()
    }

    /// The mark each tab of the shown session carries, by its badge word.
    pub fn tab_marks(&self) -> Vec<Option<&'static str>> {
        let model = self.hosts[self.active].handle.snapshot();
        model
            .view()
            .map(|v| {
                v.windows
                    .iter()
                    .map(|w| {
                        self.tab_mark(self.active, w, &model)
                            .map(|(_, badge)| badge)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The badge a pane shows: done only until it's been seen, and only states the
    /// settings show on tabs.
    pub(super) fn mark_for(
        &self,
        host: usize,
        block: &str,
        state: Option<&BlockState>,
    ) -> Option<(ProgramState, &'static str)> {
        let settings = &self.settings.notifications;
        if !settings.activity_in_tabs {
            return None;
        }
        let record = state?.status()?;
        if !settings.for_state(record.state).tabs {
            return None;
        }
        match record.state {
            ProgramState::Idle => None,
            ProgramState::Done if !self.tracker.is_unseen(&(host, block.to_owned())) => None,
            state => Some((state, record.badge())),
        }
    }

    pub(super) fn tab_mark(
        &self,
        host: usize,
        window: &ViewWindow,
        model: &HostModel,
    ) -> Option<(ProgramState, &'static str)> {
        window
            .blocks()
            .filter_map(|b| {
                let id = b.block_id.as_deref()?;
                self.mark_for(host, id, model.blocks.get(id))
            })
            .max_by_key(|(state, _)| *state)
    }

    pub(super) fn session_mark(
        &self,
        host: usize,
        session: &str,
        model: &HostModel,
    ) -> Option<(ProgramState, &'static str)> {
        model
            .blocks
            .iter()
            .filter(|(_, b)| b.session_id == session)
            .filter_map(|(id, b)| self.mark_for(host, id, Some(b)))
            .max_by_key(|(state, _)| *state)
    }

    /// The pane is on screen: its host, session and tab are showing.
    fn on_screen(&self, host: usize, session: &str, block: &str, model: &HostModel) -> bool {
        host == self.active
            && model.selected.as_deref() == Some(session)
            && model
                .views
                .get(session)
                .and_then(|v| v.active_window())
                .is_some_and(|w| w.block(block).is_some())
    }

    /// Turn new program reports into notifications and sounds, and clear what's been seen.
    pub(super) fn process_activity(&mut self, window: &mut Window) {
        let focused = window.is_window_active();
        let settings = self.settings.notifications.clone();
        let models: Vec<HostModel> = self.hosts.iter().map(|h| h.handle.snapshot()).collect();
        self.learn_tabs(&models);
        for (host, model) in models.iter().enumerate() {
            for change in self.tracker.update(host, model) {
                let in_view =
                    focused && self.on_screen(host, &change.session, &change.block, model);
                let toggles = settings.for_state(change.record.state);
                // A muted tab keeps its dot but makes no noise.
                if in_view
                    || self.pane_muted(host, &change.block, model)
                    || !matches!(
                        change.record.state,
                        ProgramState::Blocked
                            | ProgramState::Error
                            | ProgramState::Done
                            | ProgramState::Working
                    )
                {
                    continue;
                }
                if settings.system_notifications
                    && toggles.notify
                    && let Some(notifier) = &self.notifier
                {
                    let record = &change.record;
                    let pane = model
                        .views
                        .get(&change.session)
                        .and_then(|v| v.window_of(&change.block)?.block(&change.block).cloned())
                        .map(|b| titles::pane_title(&b, model.blocks.get(&change.block)))
                        .unwrap_or_else(|| "Shell".into());
                    let session = model
                        .session(&change.session)
                        .map(|s| s.label.clone())
                        .unwrap_or_default();
                    let summary = match &record.app {
                        Some(app) => format!("{app}: {}", record.badge()),
                        None => record.badge().to_owned(),
                    };
                    let mut body = record
                        .msg
                        .clone()
                        .or_else(|| record.title.clone())
                        .unwrap_or_default();
                    body = if body.is_empty() {
                        format!("{pane} · {session}")
                    } else {
                        format!("{body}\n{pane} · {session}")
                    };
                    notifier.show(Note {
                        key: note_key(host, &change.session, &change.block),
                        summary,
                        body,
                        urgency: match record.state {
                            ProgramState::Blocked | ProgramState::Error => Urgency::Critical,
                            _ => Urgency::Normal,
                        },
                    });
                }
                if settings.sounds && toggles.sound {
                    let cue = match change.record.state {
                        ProgramState::Blocked => Some(Cue::Blocked),
                        ProgramState::Error => Some(Cue::Error),
                        ProgramState::Done => Some(Cue::Done),
                        _ => None,
                    };
                    if let Some(cue) = cue {
                        sound::play(cue, settings.sound_volume);
                    }
                }
            }
            // Notifications programs send themselves (OSC 9 and OSC 777).
            let notes = std::mem::take(
                &mut self.hosts[host]
                    .handle
                    .model
                    .lock()
                    .expect("host model")
                    .notifications,
            );
            for note in notes {
                let in_view =
                    focused && self.on_screen(host, &note.session_id, &note.block_id, model);
                if in_view
                    || !settings.terminal_notifications
                    || self.pane_muted(host, &note.block_id, model)
                {
                    continue;
                }
                if let Some(notifier) = &self.notifier {
                    let summary = if note.title.is_empty() {
                        model
                            .session(&note.session_id)
                            .map(|s| s.label.clone())
                            .unwrap_or_else(|| "rexia".into())
                    } else {
                        note.title.clone()
                    };
                    notifier.show(Note {
                        key: note_key(host, &note.session_id, &note.block_id),
                        summary,
                        body: note.body.clone(),
                        urgency: Urgency::Normal,
                    });
                }
            }
        }
        // What's on screen in a focused window has been seen.
        if focused
            && let Some(model) = models.get(self.active)
            && let Some(session) = model.selected.clone()
            && let Some(window_view) = model.view().and_then(|v| v.active_window())
        {
            for block in window_view.blocks().filter_map(|b| b.block_id.clone()) {
                if self.tracker.seen(&(self.active, block.clone()))
                    && let Some(notifier) = &self.notifier
                {
                    notifier.withdraw(note_key(self.active, &session, &block));
                }
            }
        }
        if let Some(notifier) = &self.notifier {
            notifier.badge(if settings.badge {
                self.badge_count() as u32
            } else {
                0
            });
        }
    }

    fn activity_rows(&self) -> Vec<Row> {
        let settings = &self.settings.notifications;
        let mut rows = Vec::new();
        for (host, entry) in self.hosts.iter().enumerate() {
            let model = entry.handle.snapshot();
            let mut placed = std::collections::HashSet::new();
            for (session, view) in &model.views {
                let session_label = model
                    .session(session)
                    .map(|s| s.label.clone())
                    .unwrap_or_default();
                for window in &view.windows {
                    let mut panes: Vec<(String, Option<Mark>, ProgramState)> = window
                        .blocks()
                        .filter_map(|b| {
                            let id = b.block_id.clone()?;
                            let state = model.blocks.get(&id);
                            let reported = state.and_then(|s| s.status()).map(|r| r.state)?;
                            let mark = self.mark_for(host, &id, state);
                            let keep = mark.is_some()
                                || (settings.include_idle && reported == ProgramState::Idle);
                            keep.then_some((id, mark, reported))
                        })
                        .collect();
                    if panes.is_empty() {
                        continue;
                    }
                    panes.sort_by_key(|p| std::cmp::Reverse(p.2));
                    let chosen: Vec<_> = if settings.each_process {
                        panes
                    } else {
                        panes.into_iter().take(1).collect()
                    };
                    for (block, mark, state) in chosen {
                        placed.insert(block.clone());
                        let block_state = model.blocks.get(&block);
                        let record = block_state.and_then(|s| s.status());
                        let title = if settings.each_process {
                            window
                                .block(&block)
                                .map(|b| titles::pane_title(b, block_state))
                                .unwrap_or_default()
                        } else {
                            titles::tab_title(window, &model)
                        };
                        rows.push(Row {
                            host,
                            session: session.clone(),
                            session_label: session_label.clone(),
                            block: block.clone(),
                            title,
                            detail: record.and_then(|r| match (&r.app, &r.msg) {
                                (Some(app), Some(msg)) => Some(format!("{app}: {msg}")),
                                (None, Some(msg)) => Some(msg.clone()),
                                (Some(app), None) => Some(app.clone()),
                                (None, None) => None,
                            }),
                            icon: titles::program_icon(block_state),
                            mark,
                            state,
                        });
                    }
                }
            }
            // Sessions this client hasn't opened still report through block events.
            for (block, state) in &model.blocks {
                if placed.contains(block) {
                    continue;
                }
                let Some(mark) = self.mark_for(host, block, Some(state)) else {
                    continue;
                };
                rows.push(Row {
                    host,
                    session: state.session_id.clone(),
                    session_label: model
                        .session(&state.session_id)
                        .map(|s| s.label.clone())
                        .unwrap_or_default(),
                    block: block.clone(),
                    title: state
                        .current_title()
                        .map(str::to_owned)
                        .or(state.process.name.clone())
                        .unwrap_or_else(|| "Shell".into()),
                    detail: state.status().and_then(|r| r.msg.clone()),
                    icon: titles::program_icon(Some(state)),
                    mark: Some(mark),
                    state: mark.0,
                });
            }
        }
        if self.settings.interface.activity_by_priority {
            rows.sort_by(|a, b| b.state.cmp(&a.state).then(a.title.cmp(&b.title)));
        } else {
            rows.sort_by(|a, b| {
                a.session_label
                    .cmp(&b.session_label)
                    .then(b.state.cmp(&a.state))
            });
        }
        rows
    }

    pub(super) fn render_sidebar_activity(
        &self,
        below_sessions: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = self.activity_rows();
        let by_priority = self.settings.interface.activity_by_priority;
        let mut list = div().flex().flex_col().gap(px(2.));
        let mut last_session: Option<String> = None;
        for (index, row) in rows.iter().enumerate() {
            if !by_priority && last_session.as_ref() != Some(&row.session) {
                list = list.child(heading(row.session_label.clone(), theme));
                last_session = Some(row.session.clone());
            }
            let (host, session, block) = (row.host, row.session.clone(), row.block.clone());
            list = list.child(
                div()
                    .id(SharedString::from(format!("activity-{index}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(theme.radius_small())
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.overlay(0.06)))
                    .child(crate::ui::program_icons::render_icon(
                        row.icon,
                        20.,
                        theme.primary,
                        theme,
                        self.hosts.get(row.host).and_then(|host| {
                            crate::ui::program_icons::pane_colors(
                                &host.handle.snapshot(),
                                &row.block,
                            )
                        }),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(row.title.clone()),
                            )
                            .children(row.detail.clone().map(|d| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.text_muted)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(d)
                            }))
                            .when(by_priority, |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.text_muted)
                                        .child(row.session_label.clone()),
                                )
                            }),
                    )
                    .children(
                        row.mark
                            .map(|(state, badge)| activity_mark(state, badge, false, theme)),
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.select_session(host, session.clone(), cx);
                        ws.hosts[host].handle.call(
                            "session.focus_block",
                            serde_json::json!({"session_id": session, "block_id": block}),
                        );
                    })),
            );
        }
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child("No program needs you"),
            );
        }
        div()
            .id(if below_sessions {
                "sidebar-activity-section"
            } else {
                "sidebar"
            })
            .flex()
            .flex_col()
            .relative()
            .gap(px(6.))
            .when(below_sessions, |d| {
                d.flex_none()
                    .mt(px(8.))
                    .pt(px(10.))
                    .border_t(theme.border_width)
                    .border_color(theme.overlay(0.12))
                    .child(self.track_bounds("activity-below-sessions".into(), cx))
            })
            .when(!below_sessions, |d| {
                d.flex_1().min_h_0().p(px(10.)).overflow_y_scroll()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(6.))
                    .pb(px(4.))
                    .child(icon("bell").size(px(20.)).text_color(theme.primary))
                    .child(div().flex_1().child(title("Activity", theme)))
                    .child(
                        small_icon_button(
                            "activity-grouping",
                            if by_priority {
                                "layout-columns"
                            } else {
                                "alert-triangle"
                            },
                            theme.text_muted,
                            theme,
                            cx.listener(|ws, _, _, cx| {
                                ws.settings.interface.activity_by_priority =
                                    !ws.settings.interface.activity_by_priority;
                                ws.save_settings();
                                cx.notify();
                            }),
                        )
                        .tooltip(self.tip(
                            if by_priority {
                                "client.sidebar.activity.by_session"
                            } else {
                                "client.sidebar.activity.by_priority"
                            },
                            None,
                        )),
                    )
                    .child(
                        small_icon_button(
                            "activity-close",
                            if below_sessions { "x" } else { "server" },
                            theme.text_muted,
                            theme,
                            cx.listener(move |ws, _, _, cx| {
                                ws.update_settings(
                                    |s| {
                                        if below_sessions {
                                            s.interface.activity_below_sessions = false;
                                        } else {
                                            s.interface.activity_view = false;
                                        }
                                    },
                                    cx,
                                );
                            }),
                        )
                        .tooltip(self.tip("client.sidebar.activity.toggle", Some("Hide Activity"))),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_keys_round_trip() {
        let key = note_key(2, "session:1", "block:9");
        assert_eq!(
            parse_key(&key),
            Some((2, "session:1".into(), "block:9".into()))
        );
        assert_eq!(parse_key("garbage"), None);
    }
}
