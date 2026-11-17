//! ssh's questions about ssh:// hosts, answered in a dialog above everything else (even
//! Settings, where Retry is): trust a new host key, type a password or passphrase. A second
//! question waits for the first.
use super::{Workspace, preferences::button};
use crate::{
    rex::askpass::{self, Kind, Question},
    ui::{
        input::{InputEvent, TextInput},
        theme::Theme,
    },
};
use gpui::{
    AnyElement, AppContext, Context, Entity, FontWeight, IntoElement, ParentElement, Styled,
    Subscription, Window, div, prelude::*, px,
};

pub struct SshQuestion {
    question: Question,
    /// The host's name, as rexia lists it.
    host: String,
    /// The answer field, for passwords and passphrases.
    input: Option<Entity<TextInput>>,
    /// Which button Enter presses for a yes or no question: 0 no, 1 yes.
    focused: usize,
    _subscription: Option<Subscription>,
}

impl Workspace {
    /// Take ssh's questions as they come.
    pub(super) fn listen_for_ssh_questions(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Task<()> {
        let questions = askpass::questions();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(question) = questions.recv_async().await {
                if this
                    .update_in(cx, |ws, window, cx| ws.ssh_asked(question, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn ssh_asked(&mut self, question: Question, window: &mut Window, cx: &mut Context<Self>) {
        if question.kind == Kind::Notice {
            self.notice = Some(format!(
                "{}: {}",
                self.ssh_host_label(&question.host),
                question.prompt.trim()
            ));
            cx.notify();
            return;
        }
        self.ssh_questions.push_back(question);
        self.next_ssh_question(window, cx);
    }

    /// The name rexia lists a host under, from the address ssh was given.
    fn ssh_host_label(&self, address: &str) -> String {
        self.hosts
            .iter()
            .find(|h| {
                crate::rex::endpoint::Endpoint::parse(&h.config.endpoint)
                    .is_ok_and(|e| e.to_string() == address)
            })
            .map_or_else(|| address.to_owned(), |h| h.config.label.clone())
    }

    /// Show the next waiting question, once the one showing is answered.
    fn next_ssh_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ssh_open.is_some() {
            return;
        }
        let Some(question) = self.ssh_questions.pop_front() else {
            return;
        };
        let host = self.ssh_host_label(&question.host);
        let (input, subscription) = if question.kind == Kind::Secret {
            let input = cx.new(|cx| TextInput::new("", cx).secret());
            let subscription = cx.subscribe_in(
                &input,
                window,
                |ws: &mut Workspace, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Submit { .. } => ws.answer_ssh(true, window, cx),
                    InputEvent::Cancel => ws.answer_ssh(false, window, cx),
                    _ => {}
                },
            );
            let handle = gpui::Focusable::focus_handle(input.read(cx), cx);
            window.focus(&handle, cx);
            (Some(input), Some(subscription))
        } else {
            window.focus(&self.focus, cx);
            (None, None)
        };
        self.ssh_open = Some(Box::new(SshQuestion {
            question,
            host,
            input,
            focused: 0,
            _subscription: subscription,
        }));
        cx.notify();
    }

    /// Answer the question showing: yes with what was typed (or `yes`), or decline it.
    pub fn answer_ssh(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.ssh_open.take() else {
            return;
        };
        let typed = open.input.as_ref().map(|i| i.read(cx).text().to_owned());
        let answer = match (yes, open.question.kind) {
            (false, _) => None,
            (true, Kind::Confirm) => Some("yes".to_owned()),
            (true, _) => Some(typed.unwrap_or_default()),
        };
        open.question.answer(answer);
        if self.overlay.is_some() {
            window.focus(&self.focus, cx);
        } else {
            self.refocus(window, cx);
        }
        self.next_ssh_question(window, cx);
        cx.notify();
    }

    /// Keys for a yes or no question: Enter, Esc, Tab and the arrows. A password field
    /// answers its own.
    pub(super) fn ssh_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(open) = &mut self.ssh_open else {
            return false;
        };
        if open.input.is_none() {
            match key {
                "escape" => self.answer_ssh(false, window, cx),
                "enter" => {
                    let yes = open.focused == 1;
                    self.answer_ssh(yes, window, cx);
                }
                "tab" | "left" | "right" => {
                    open.focused = 1 - open.focused;
                    cx.notify();
                }
                _ => {}
            }
        }
        true
    }

    /// The question showing, if any, for tests: its host, words and kind.
    pub fn ssh_question(&self) -> Option<(String, String, Kind)> {
        let open = self.ssh_open.as_ref()?;
        Some((
            open.host.clone(),
            open.question.prompt.trim().to_owned(),
            open.question.kind,
        ))
    }

    /// Type into the question's field, for tests.
    pub fn type_ssh_answer(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(input) = self.ssh_open.as_ref().and_then(|o| o.input.clone()) {
            input.update(cx, |i, cx| i.set_text(text, cx));
        }
    }

    pub(super) fn render_ssh_question(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.ssh_open.as_ref()?;
        let confirm = open.question.kind == Kind::Confirm;
        let (no, yes) = if confirm {
            ("Don't Connect", "Connect")
        } else {
            ("Cancel", "Continue")
        };
        let ring = |d: gpui::Stateful<gpui::Div>, on: bool| {
            d.when(on, |d| d.border_2().border_color(theme.primary))
        };
        let picking = open.input.is_none();
        let panel = div()
            .id("ssh-question")
            .w(px(480.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(18.))
            .bg(theme.surface)
            .rounded(theme.radius_large())
            .when(theme.popup_borders, |d| {
                d.border(theme.border_width).border_color(theme.border)
            })
            .when(theme.popup_shadows, |d| d.shadow_lg())
            .font_family(theme.ui_font.clone())
            .text_color(theme.text)
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .child(format!("ssh asks about {}", open.host)),
            )
            .child(
                div()
                    .id("ssh-question-text")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(open.question.prompt.trim().to_owned()),
            )
            .children(open.input.clone())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        ring(
                            button("ssh-no", no, false, theme),
                            picking && open.focused == 0,
                        )
                        .on_click(
                            cx.listener(|ws, _, window, cx| ws.answer_ssh(false, window, cx)),
                        ),
                    )
                    .child(
                        ring(
                            button("ssh-yes", yes, true, theme),
                            picking && open.focused == 1,
                        )
                        .on_click(cx.listener(|ws, _, window, cx| ws.answer_ssh(true, window, cx))),
                    ),
            );
        Some(
            div()
                .id("ssh-question-scrim")
                .occlude()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(96.))
                .bg(gpui::hsla(0., 0., 0., 0.35))
                // Answering takes a button, so a stray click doesn't decline.
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(panel)
                .into_any_element(),
        )
    }

    /// Retry a host now: forget a declined question and reconnect.
    pub fn retry_host(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(host) = self.hosts.get(index) else {
            return;
        };
        if let Ok(endpoint) = crate::rex::endpoint::Endpoint::parse(&host.config.endpoint) {
            askpass::retry(&endpoint.to_string());
        }
        host.handle.send(crate::rex::host::HostCommand::Reconnect);
        cx.notify();
    }

    /// Move a host up (-1) or down (1) the list; the order is kept in settings.
    pub fn move_host(&mut self, index: usize, by: isize, cx: &mut Context<Self>) {
        let to = index.saturating_add_signed(by);
        if index >= self.hosts.len() || to >= self.hosts.len() || to == index {
            return;
        }
        let mut order: Vec<String> = self.hosts.iter().map(|h| h.config.id.clone()).collect();
        let id = order.remove(index);
        order.insert(to, id);
        self.update_settings(|s| s.host_order = order, cx);
        self.order_hosts();
        cx.notify();
    }

    /// Put the hosts in the order settings keep; hosts it doesn't name go last.
    pub(super) fn order_hosts(&mut self) {
        let order = &self.settings.host_order;
        if order.is_empty() {
            return;
        }
        // While hosts come and go, `active` can point past the end for a moment; the
        // caller puts it right then.
        let active = self.hosts.get(self.active).map(|h| h.config.id.clone());
        self.hosts.sort_by_key(|h| {
            order
                .iter()
                .position(|id| *id == h.config.id)
                .unwrap_or(usize::MAX)
        });
        if let Some(active) = active {
            self.active = self
                .hosts
                .iter()
                .position(|h| h.config.id == active)
                .unwrap_or(0);
        }
    }
}
