//! Settings › Client & Hosts › SSH: the agent ssh asks for keys for ssh:// hosts.
use super::{
    Workspace,
    preferences::{SettingsPage, button, row},
};
use crate::{rex::ssh_agent, ui::theme::Theme};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use std::path::PathBuf;

/// The socket ssh's own choice uses, when the environment names one.
fn session_socket() -> Option<PathBuf> {
    std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from)
}

impl Workspace {
    /// Use `socket` as the SSH agent for ssh:// hosts from now on, or ssh's own choice.
    pub fn choose_ssh_agent(&mut self, socket: Option<PathBuf>, cx: &mut Context<Self>) {
        self.update_settings(|s| s.ssh_agent = socket, cx);
        self.refresh_ssh_agent_status(cx);
    }

    /// Ask the chosen agent, or ssh's default one, how many keys it holds.
    pub fn refresh_ssh_agent_status(&mut self, cx: &mut Context<Self>) {
        let chosen = self.settings.ssh_agent.clone();
        cx.spawn(async move |this, cx| {
            let asked = chosen.clone();
            let status = cx
                .background_executor()
                .spawn(async move {
                    match asked.or_else(session_socket) {
                        Some(socket) => ssh_agent::status(&socket),
                        None => "No agent in this session (SSH_AUTH_SOCK isn't set); ssh uses \
                                 IdentityAgent from ~/.ssh/config, if it names one"
                            .into(),
                    }
                })
                .await;
            let _ = this.update(cx, |ws, cx| {
                ws.ssh_agent_status = Some((chosen, status));
                cx.notify();
            });
        })
        .detach();
    }

    /// The chosen agent's status, once asked, for the UI and tests.
    pub fn ssh_agent_status(&self) -> Option<&str> {
        self.ssh_agent_status
            .as_ref()
            .filter(|(socket, _)| *socket == self.settings.ssh_agent)
            .map(|(_, status)| status.as_str())
    }

    pub(super) fn ssh_rows(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let chosen = self.settings.ssh_agent.clone();
        let mut choices = vec![(
            "ssh's default".to_owned(),
            None,
            "SSH_AUTH_SOCK, or IdentityAgent in ~/.ssh/config".to_owned(),
        )];
        for agent in ssh_agent::known_agents() {
            let path = agent.socket.display().to_string();
            choices.push((agent.name.to_owned(), Some(agent.socket), path));
        }
        if let Some(socket) = &chosen
            && !choices.iter().any(|(_, s, _)| s.as_ref() == Some(socket))
        {
            choices.push((
                "Custom socket".to_owned(),
                Some(socket.clone()),
                socket.display().to_string(),
            ));
        }
        let buttons = choices
            .into_iter()
            .enumerate()
            .map(|(index, (name, socket, detail))| {
                let selected = socket == chosen;
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        button(format!("ssh-agent-{index}"), &name, selected, theme).on_click(
                            cx.listener(move |ws, _, _, cx| {
                                ws.choose_ssh_agent(socket.clone(), cx)
                            }),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.text_muted)
                            .child(detail),
                    )
            });
        let status = self
            .ssh_agent_status()
            .map(str::to_owned)
            .unwrap_or_else(|| "Asking the agent…".into());
        let answers = status.starts_with("Answers");
        vec![
            div()
                .pt(px(16.))
                .pb(px(4.))
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child("SSH")
                .into_any_element(),
            row(
                "ssh-agent",
                "SSH agent",
                "Where ssh finds keys for ssh:// hosts. New connections use it.",
                theme,
            )
            .into_any_element(),
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(12.))
                .pb(px(8.))
                .children(buttons)
                .into_any_element(),
            self.indexed_field_row(
                "ssh-agent-socket",
                "SSH agent socket",
                "The path of a custom agent's socket. Enter uses it.",
                &page.ssh_socket,
                theme,
                cx,
            ),
            div()
                .id("ssh-agent-status")
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(12.))
                .py(px(8.))
                .child(div().text_size(px(13.)).child("SSH agent status"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if answers {
                            theme.text_muted
                        } else {
                            theme.error
                        })
                        .child(status),
                )
                .into_any_element(),
        ]
    }
}
