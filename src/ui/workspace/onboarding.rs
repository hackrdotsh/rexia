//! Open Onboarding Session…: Rex's setup walkthrough. The server keeps an onboarding
//! block's step, so every client showing it stays on the same step; rexia draws the steps:
//! welcome, how rexia looks, keys, coding agents, and done.
use super::{
    Workspace,
    preferences::{button, row},
};
use crate::{
    rex::{
        onboarding::{self, CREATOR, STEPS},
        view::Block,
    },
    settings::{PaneStyle, ToolbarLayout},
    ui::{icons::icon, theme::Theme},
};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, ParentElement, SharedString,
    Styled, div, prelude::*, px,
};
use serde_json::{Value, json};

/// Whether a block is an onboarding walkthrough rather than a terminal.
pub fn is_onboarding(block: &Block) -> bool {
    block.creator_name == CREATOR || block.flavor == onboarding::FLAVOR
}

impl Workspace {
    /// `client.onboarding`: show the onboarding session, opening one if there's none.
    pub fn open_onboarding(&mut self, cx: &mut Context<Self>) {
        let host = self.active;
        let model = self.model();
        if let Some(existing) = model.sessions.iter().find(|s| s.label == "Onboarding") {
            let id = existing.session_id.clone();
            self.select_session(host, id, cx);
            return;
        }
        let handle = self.host().handle.clone();
        cx.spawn(async move |this, cx| {
            let created = handle
                .request(
                    "session.create",
                    json!({"label": "Onboarding", "initial_windows": [{"layout": onboarding::layout()}]}),
                )
                .await;
            let _ = this.update(cx, |ws, cx| match created {
                Ok(reply) => {
                    let id = reply["session"]["session_id"]
                        .as_str()
                        .or(reply["session_id"].as_str())
                        .unwrap_or_default()
                        .to_owned();
                    ws.select_session(host, id, cx);
                }
                Err(error) => {
                    ws.notice = Some(format!("Couldn't open onboarding: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The step an onboarding block is on: the newest of what rexia was told and what
    /// the server's events said.
    pub fn onboarding_step(&self, block: &str) -> String {
        let model = self.model();
        let local = self.onboarding.get(block);
        let event = model.onboarding.get(block);
        match (local, event) {
            (Some(a), Some(b)) if b.0 > a.0 => b.1.clone(),
            (Some(a), _) => a.1.clone(),
            (None, Some(b)) => b.1.clone(),
            (None, None) => String::new(),
        }
    }

    /// Whether the server has told rexia this block's step yet, for tests.
    pub fn has_onboarding_answer(&self, block: &str) -> bool {
        self.onboarding.contains_key(block) || self.model().onboarding.contains_key(block)
    }

    fn remember_onboarding(&mut self, block: &str, reply: &Value) {
        let state = (
            reply["generation"].as_u64().unwrap_or(0),
            reply["step"].as_str().unwrap_or_default().to_owned(),
        );
        let known = self.onboarding.entry(block.to_owned()).or_default();
        if state.0 >= known.0 {
            *known = state;
        }
    }

    /// Ask the server which step an onboarding block is on, once.
    pub(super) fn fetch_onboarding(&mut self, session: &str, block: &str, cx: &mut Context<Self>) {
        if !self.onboarding_asked.insert(block.to_owned()) {
            return;
        }
        self.call_onboarding(session, block, "state", json!({}), cx);
    }

    /// Move an onboarding block to `step`; every client showing it follows.
    pub fn set_onboarding_step(
        &mut self,
        session: &str,
        block: &str,
        step: &str,
        cx: &mut Context<Self>,
    ) {
        self.call_onboarding(session, block, "set_step", json!({"step": step}), cx);
    }

    fn call_onboarding(
        &mut self,
        session: &str,
        block: &str,
        method: &str,
        args: Value,
        cx: &mut Context<Self>,
    ) {
        let handle = self.host().handle.clone();
        let payload = json!({"session_id": session, "block_id": block, "args": args});
        let method = format!("{CREATOR}.{method}");
        let block = block.to_owned();
        cx.spawn(async move |this, cx| {
            let reply = handle.request(&method, payload).await;
            let _ = this.update(cx, |ws, cx| {
                match reply {
                    Ok(reply) => ws.remember_onboarding(&block, &reply),
                    Err(error) => ws.notice = Some(format!("Onboarding: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// One onboarding block, drawn as the walkthrough's current step.
    pub(super) fn render_onboarding(
        &self,
        session: &str,
        block: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let index = onboarding::index(&self.onboarding_step(block));
        let (title, body, extra) = self.onboarding_page(index, session, theme, cx);
        let go = |to: usize| {
            let (session, block) = (session.to_owned(), block.to_owned());
            cx.listener(move |ws: &mut Workspace, _: &gpui::ClickEvent, _, cx| {
                ws.set_onboarding_step(&session, &block, STEPS[to], cx)
            })
        };
        let dots = (0..STEPS.len()).map(|i| {
            div().size(px(8.)).rounded_full().bg(if i == index {
                theme.primary
            } else {
                theme.overlay(0.2)
            })
        });
        let mut nav = div().flex().gap(px(8.));
        if index > 0 {
            nav =
                nav.child(button("onboarding-back", "Back", false, theme).on_click(go(index - 1)));
        }
        if index + 1 < STEPS.len() {
            nav = nav.child(button("onboarding-next", "Next", true, theme).on_click(go(index + 1)));
        }
        div()
            .id(SharedString::from(format!("onboarding-{block}")))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(28.))
            .overflow_y_scroll()
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("command").size(px(22.)).text_color(theme.primary))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(theme.text_muted)
                    .child(body),
            )
            .children(extra)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt(px(8.))
                    .child(div().flex().gap(px(6.)).children(dots))
                    .child(nav),
            )
            .into_any_element()
    }

    /// A step's title, words, and controls.
    fn onboarding_page(
        &self,
        index: usize,
        session: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> (&'static str, &'static str, Vec<AnyElement>) {
        match STEPS[index] {
            "look" => {
                let i = &self.settings.interface;
                let layout = |value: ToolbarLayout, label: &'static str| {
                    button(
                        format!("onboarding-layout-{label}"),
                        label,
                        i.toolbar_layout == value,
                        theme,
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.update_settings(|s| s.interface.toolbar_layout = value, cx)
                    }))
                };
                let style = |value: PaneStyle, label: &'static str| {
                    button(
                        format!("onboarding-style-{label}"),
                        label,
                        i.pane_style == value,
                        theme,
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.update_settings(|s| s.interface.pane_style = value, cx)
                    }))
                };
                (
                    "How rexia looks",
                    "Colors, corners, borders and the interface font follow your Noctalia theme, \
                     live. Choose where tabs go and how close panes sit; Settings has the rest.",
                    vec![
                        row(
                            "onboarding-tabs",
                            "Tabs",
                            "Above the panes, or down the sidebar.",
                            theme,
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(layout(ToolbarLayout::Above, "Above"))
                                .child(layout(ToolbarLayout::Left, "Left"))
                                .child(layout(ToolbarLayout::Right, "Right")),
                        )
                        .into_any_element(),
                        row("onboarding-panes", "Panes", "Space between panes.", theme)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .child(style(PaneStyle::Comfortable, "Comfortable"))
                                    .child(style(PaneStyle::Compact, "Compact")),
                            )
                            .into_any_element(),
                    ],
                )
            }
            "keys" => {
                let config = self.keymap_config_path();
                let mut copy = row(
                    "onboarding-config",
                    "Your keys",
                    config
                        .as_deref()
                        .unwrap_or("The server hasn't said where its config file is."),
                    theme,
                );
                if let Some(path) = config {
                    copy = copy.child(
                        button("onboarding-copy-config", "Copy Path", false, theme).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(path.clone()))
                            }),
                        ),
                    );
                }
                (
                    "Keys",
                    "Rex's key bindings live in its init.lua, shared by every client. rexia uses \
                     ctrl+shift where the Mac uses cmd, as Ghostty does. Bring tmux, zellij or \
                     Ghostty habits over with rex.bind there, or give one of rexia's actions a \
                     key with Change Shortcut.",
                    vec![
                        copy.into_any_element(),
                        row(
                            "onboarding-shortcut",
                            "Change Shortcut",
                            "Pick an action and press its new key.",
                            theme,
                        )
                        .child(
                            button(
                                "onboarding-change-shortcut",
                                "Change Shortcut…",
                                false,
                                theme,
                            )
                            .on_click(
                                cx.listener(|ws, _, window, cx| ws.edit_shortcuts(window, cx)),
                            ),
                        )
                        .into_any_element(),
                    ],
                )
            }
            "agents" => {
                let message = self.agents_state().message.clone();
                let mut rows = vec![
                    row(
                        "onboarding-agents",
                        "Install Rex for Agents",
                        "Teach Claude Code, Codex, OpenCode and Pi to use Rex: a skill and session hooks.",
                        theme,
                    )
                    .child(button("onboarding-agents-install", "Install", true, theme).on_click(
                        cx.listener(|ws, _, _, cx| ws.setup_agents(None, cx)),
                    ))
                    .into_any_element(),
                ];
                if let Some(message) = message {
                    rows.push(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.text_muted)
                            .child(message)
                            .into_any_element(),
                    );
                }
                (
                    "Coding agents",
                    "Agents running in a Rex terminal can open panes, read output and name \
                     sessions once they know how.",
                    rows,
                )
            }
            "done" => {
                let session = session.to_owned();
                (
                    "You're set",
                    "Open the command palette with ctrl+shift+p for everything rexia does. \
                     Settings (ctrl+,) has the rest. Close this session when you're done.",
                    vec![
                        row(
                            "onboarding-close",
                            "Close onboarding",
                            "End this session.",
                            theme,
                        )
                        .child(
                            button("onboarding-close-session", "Close", true, theme).on_click(
                                cx.listener(move |ws, _, _, _| {
                                    ws.host()
                                        .handle
                                        .call("session.destroy", json!({"session_id": session}));
                                }),
                            ),
                        )
                        .into_any_element(),
                    ],
                )
            }
            _ => (
                "Welcome to Rex on Linux",
                "rexia is a Rex client for Linux. Sessions hold tabs, tabs hold panes, and \
                 every one keeps running on the server when rexia closes. This walkthrough \
                 sets up how rexia looks, your keys, and your coding agents.",
                Vec::new(),
            ),
        }
    }

    /// The server's config file path, from the key bindings it sent.
    fn keymap_config_path(&self) -> Option<String> {
        self.model()
            .keymap
            .as_ref()?
            .get("config")?
            .get("path")?
            .as_str()
            .map(str::to_owned)
    }
}
