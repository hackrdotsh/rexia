//! Desktop notifications about a server's built-in Tailscale, as Rex for Mac sends them:
//! when it needs you to sign in, when a tailnet admin must approve the device, and when it
//! stops or reports a problem. Each has its own switch under Settings › Notifications. A
//! sign-in or approval notice is taken back once Tailscale moves on; clicking one opens the
//! sign-in link when there is one, else Settings › Server.
use super::{SettingsSection, Workspace};
use crate::notify::{Note, Urgency};
use gpui::{Context, Window};
use serde_json::Value;

/// What rexia last saw of a host's Tailscale: its state, and the error it reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    state: String,
    error: Option<String>,
}

/// The notification keys rexia uses for a host's Tailscale; clicks on them come back here.
fn key(host: usize, what: &str) -> String {
    format!("tailscale\u{1f}{host}\u{1f}{what}")
}

/// A Tailscale notification's host, from its key.
pub(super) fn parse_key(key: &str) -> Option<usize> {
    key.strip_prefix("tailscale\u{1f}")?
        .split('\u{1f}')
        .next()?
        .parse()
        .ok()
}

impl Workspace {
    /// Compare each host's Tailscale with what rexia saw before, and notify about changes.
    pub(super) fn check_tailscale(&mut self) {
        for host in 0..self.hosts.len() {
            let status = self.hosts[host].handle.snapshot().server["tailscale"].clone();
            if !status.is_object() {
                continue;
            }
            let now = Seen {
                state: status["state"].as_str().unwrap_or("disabled").to_owned(),
                error: status["error"]
                    .as_str()
                    .filter(|e| !e.trim().is_empty())
                    .map(str::to_owned),
            };
            let before = self.tailscale_seen.insert(host, now.clone());
            if before.as_ref() == Some(&now) {
                continue;
            }
            self.tailscale_changed(host, before.unwrap_or_default(), &now, &status);
        }
    }

    fn tailscale_changed(&self, host: usize, before: Seen, now: &Seen, status: &Value) {
        let Some(notifier) = &self.notifier else {
            return;
        };
        let settings = &self.settings.notifications;
        let label = self.hosts[host].config.label.clone();
        // What was asked of you is done, or no longer asked.
        if before.state == "needs_login" && now.state != "needs_login" {
            notifier.withdraw(key(host, "sign-in"));
        }
        if before.state == "needs_machine_approval" && now.state != "needs_machine_approval" {
            notifier.withdraw(key(host, "approval"));
        }
        match now.state.as_str() {
            "needs_login" if before.state != "needs_login" && settings.tailscale_sign_in => {
                notifier.show(Note {
                    key: key(host, "sign-in"),
                    summary: "Tailscale needs you to sign in".into(),
                    body: if status["auth_url"].is_string() {
                        format!("Rex on {label} is waiting. Click to open the sign-in page.")
                    } else {
                        format!("Rex on {label} is waiting. Sign in from Settings › Server.")
                    },
                    urgency: Urgency::Normal,
                })
            }
            "needs_machine_approval"
                if before.state != "needs_machine_approval" && settings.tailscale_approval =>
            {
                notifier.show(Note {
                    key: key(host, "approval"),
                    summary: "Tailscale needs an admin's approval".into(),
                    body: format!(
                        "A tailnet admin must approve Rex on {label} before other devices can reach it."
                    ),
                    urgency: Urgency::Normal,
                })
            }
            _ => {}
        }
        // A new problem, or Tailscale stopping with one.
        if let Some(error) = &now.error
            && before.error.as_ref() != Some(error)
            && settings.tailscale_failures
        {
            let stopped = now.state == "disabled" && before.state != "disabled";
            notifier.show(Note {
                key: key(host, "failure"),
                summary: if stopped {
                    "Tailscale stopped".into()
                } else {
                    "Tailscale reports a problem".into()
                },
                body: format!("Rex on {label}: {error}"),
                urgency: Urgency::Critical,
            });
        }
    }

    /// A Tailscale notification was clicked: open the sign-in page if Tailscale has one
    /// waiting, else Settings › Server for that host.
    pub(super) fn tailscale_note_clicked(
        &mut self,
        host: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if host >= self.hosts.len() {
            return;
        }
        let status = self.hosts[host].handle.snapshot().server["tailscale"].clone();
        if status["state"] == "needs_login"
            && let Some(url) = status["auth_url"].as_str()
        {
            (self.link_opener.clone())(url, cx);
            return;
        }
        if host != self.active {
            self.switch_host(host, cx);
        }
        self.open_settings(window, cx);
        self.show_settings_section(SettingsSection::Server, window, cx);
        window.activate_window();
    }
}
