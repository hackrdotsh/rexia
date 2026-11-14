//! Settings › Server: the addresses other devices reach the server at, and built-in
//! Tailscale: turning it on and off, signing in and out, the device's name and who may
//! connect.
use super::{
    Workspace,
    overlay::Overlay,
    preferences::{button, row},
    server_status::ServerReportState,
};
use crate::ui::theme::Theme;
use gpui::{
    AnyElement, ClipboardItem, Context, IntoElement, ParentElement, Styled, div, prelude::*, px,
};
use serde_json::{Value, json};

/// One way to reach the server from another device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAddress {
    /// What to give a client, such as `http://10.0.0.5:7297`.
    pub address: String,
    /// How the server listens there: its endpoint, or built-in Tailscale.
    pub source: String,
    /// Who it lets in, in words.
    pub admits: String,
    /// The address works now; false while its certificate is still coming.
    pub ready: bool,
}

/// Who a listener lets in, in words. Values the server adds later show as they are.
pub fn admission(value: &str) -> String {
    match value {
        "peer_credentials" => "Accounts on this computer, by their user".into(),
        "anonymous" => "Anyone who can reach it, with full access".into(),
        "tailscale_owner" => "Only your Tailscale account".into(),
        "tailscale_identified" => "Anyone Tailscale can identify".into(),
        "login_server" => "Accounts the login server signs in".into(),
        other => other.into(),
    }
}

/// The addresses other devices can use: network listeners and built-in Tailscale's.
/// Unix sockets and named pipes only reach this computer, so they aren't listed.
pub fn remote_addresses(status: &Value) -> Vec<RemoteAddress> {
    let mut found = Vec::new();
    for listener in status["listeners"].as_array().into_iter().flatten() {
        let transport = listener["transport"].as_str().unwrap_or_default();
        if matches!(transport, "unix" | "named_pipe") {
            continue;
        }
        let endpoint = listener["endpoint"].as_str().unwrap_or_default();
        // The bound address shows the port an endpoint like :0 got.
        let bound = listener["addresses"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .unwrap_or(endpoint);
        let address = if bound.contains("://") {
            bound.to_owned()
        } else if transport == "tcp" {
            format!("http://{bound}")
        } else {
            bound.to_owned()
        };
        found.push(RemoteAddress {
            address,
            source: endpoint.to_owned(),
            admits: admission(listener["admission"].as_str().unwrap_or("unknown")),
            ready: true,
        });
    }
    let tailscale = &status["tailscale"];
    if tailscale["state"] == "running" {
        let url = tailscale["https"]["url"].as_str().map(str::to_owned);
        let ready = tailscale["https"]["state"] == "available";
        let name = tailscale["dns_name"].as_str().unwrap_or("this device");
        found.push(RemoteAddress {
            address: url.unwrap_or_else(|| format!("https://{name}")),
            source: "Built-in Tailscale".into(),
            admits: if tailscale["allow_tailnet_members"] == true {
                "Anyone your tailnet's access rules allow".into()
            } else {
                "Only your Tailscale account".into()
            },
            ready,
        });
    }
    found
}

/// Built-in Tailscale's state, in words.
pub fn tailscale_state(tailscale: &Value) -> String {
    let account = tailscale["account"]["login_name"].as_str();
    let error = tailscale["error"].as_str();
    match tailscale["state"].as_str().unwrap_or("disabled") {
        "disabled" => match error {
            Some(error) => format!("Off: {error}"),
            None => "Off".into(),
        },
        "needs_login" => {
            if tailscale["auth_url"].as_str().is_some() {
                "Waiting for you to sign in".into()
            } else {
                "Needs signing in; asking Tailscale for a link…".into()
            }
        }
        "needs_machine_approval" => "Waiting for a tailnet admin to approve this device".into(),
        "running" => {
            let name = tailscale["dns_name"].as_str().unwrap_or("this device");
            let https = if tailscale["https"]["state"] == "available" {
                ""
            } else {
                ", waiting for its HTTPS certificate"
            };
            match account {
                Some(account) => format!("On as {name}, signed in as {account}{https}"),
                None => format!("On as {name}{https}"),
            }
        }
        _ => "Starting…".into(),
    }
}

impl Workspace {
    /// Built-in Tailscale's status, the newer of the report and live events; `None` when
    /// the server can't run it.
    pub fn tailscale_status(&self) -> Option<Value> {
        let model = self.model();
        let report = match &self.server_report {
            ServerReportState::Ready(report) => Some(report),
            _ => None,
        };
        let reported = report
            .and_then(|r| r.status.as_ref().ok())
            .map(|s| s["tailscale"].clone())
            .filter(Value::is_object);
        let live = model.server["tailscale"].clone();
        let live_newer = report.is_none_or(|r| model.tailscale_revision > r.tailscale_revision);
        if live_newer && live.is_object() {
            Some(live)
        } else {
            reported
        }
    }

    /// Call a `server.tailscale.*` method; its answer, or the error, shows in the section.
    pub fn tailscale_call(&mut self, method: &'static str, args: Value, cx: &mut Context<Self>) {
        let handle = self.host().handle.clone();
        self.tailscale_error = None;
        self.tailscale_busy = Some(method);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = handle.request(method, args).await;
            let _ = this.update(cx, |ws, cx| {
                ws.tailscale_busy = None;
                if let Err(error) = result {
                    ws.tailscale_error = Some(error.to_string());
                }
                ws.refresh_server_report(cx);
            });
        })
        .detach();
    }

    /// The Tailscale fields: preferred hostname, control server and tags.
    fn tailscale_fields(&self, cx: &gpui::App) -> Option<(String, String, Vec<String>)> {
        let Some(Overlay::Settings(page)) = &self.overlay else {
            return None;
        };
        let text = |input: &gpui::Entity<crate::ui::input::TextInput>| {
            input.read(cx).text().trim().to_owned()
        };
        let tags = text(&page.ts_tags)
            .split([',', ' '])
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
        Some((text(&page.ts_hostname), text(&page.ts_control), tags))
    }

    /// Fill in the Tailscale fields, as typing would, for tests.
    pub fn set_tailscale_fields(
        &mut self,
        hostname: &str,
        tags: &str,
        control: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(Overlay::Settings(page)) = &self.overlay else {
            return;
        };
        for (input, text) in [
            (page.ts_hostname.clone(), hostname),
            (page.ts_tags.clone(), tags),
            (page.ts_control.clone(), control),
        ] {
            input.update(cx, |input, cx| input.set_text(text.to_owned(), cx));
        }
    }

    /// The last Tailscale call's error, for tests.
    pub fn tailscale_error(&self) -> Option<&str> {
        self.tailscale_error.as_deref()
    }

    /// Serve over Tailscale, or stop.
    pub fn set_tailscale(&mut self, on: bool, cx: &mut Context<Self>) {
        if !on {
            self.tailscale_call("server.tailscale.disable", json!({}), cx);
            return;
        }
        let (hostname, control, tags) = self.tailscale_fields(cx).unwrap_or_default();
        let members = self.tailscale_members;
        if !tags.is_empty() && !members {
            self.tailscale_error = Some(
                "A tagged device has no owner, so it needs access for tailnet members.".into(),
            );
            cx.notify();
            return;
        }
        let mut args = json!({"allow_tailnet_members": members});
        if !hostname.is_empty() {
            args["hostname"] = json!(hostname);
        }
        if !control.is_empty() {
            args["control_url"] = json!(control);
        }
        if !tags.is_empty() {
            args["advertise_tags"] = json!(tags);
        }
        self.tailscale_call("server.tailscale.enable", args, cx);
    }

    /// Limit access to your Tailscale account, or let tailnet members in. While Tailscale
    /// runs this applies at once, without dropping connections.
    pub fn set_tailscale_members(&mut self, members: bool, cx: &mut Context<Self>) {
        self.tailscale_members = members;
        let running = self
            .tailscale_status()
            .is_some_and(|t| t["state"] != "disabled");
        if running {
            self.tailscale_call(
                "server.tailscale.enable",
                json!({"allow_tailnet_members": members}),
                cx,
            );
        } else {
            cx.notify();
        }
    }

    pub(super) fn remote_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let report = match &self.server_report {
            ServerReportState::Ready(report) => report,
            _ => return Vec::new(),
        };
        let Ok(status) = &report.status else {
            return Vec::new();
        };
        let mut status = status.clone();
        if let Some(tailscale) = self.tailscale_status() {
            status["tailscale"] = tailscale;
        }
        let heading = |text: &str| {
            div()
                .pt(px(16.))
                .pb(px(4.))
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(text.to_owned())
                .into_any_element()
        };
        let mut rows = vec![heading("Remote Address")];
        let addresses = remote_addresses(&status);
        if addresses.is_empty() {
            rows.push(
                row(
                    "remote-none",
                    "Only this computer",
                    "The server listens on its local socket only. Turn on Tailscale below, \
                     or start it with --extra-listen to reach it from other devices.",
                    theme,
                )
                .into_any_element(),
            );
        }
        for (index, remote) in addresses.into_iter().enumerate() {
            let copy = remote.address.clone();
            let detail = format!(
                "{} · {}{}",
                remote.source,
                remote.admits,
                if remote.ready {
                    ""
                } else {
                    " · waiting for its HTTPS certificate"
                }
            );
            rows.push(
                row(format!("remote-{index}"), &remote.address, &detail, theme)
                    .child(
                        button(format!("remote-copy-{index}"), "Copy", false, theme).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            }),
                        ),
                    )
                    .into_any_element(),
            );
        }

        let Some(tailscale) = status["tailscale"]
            .as_object()
            .map(|_| &status["tailscale"])
        else {
            return rows;
        };
        rows.push(heading("Tailscale"));
        let state = tailscale["state"].as_str().unwrap_or("disabled");
        let on = state != "disabled";
        let local = report.local;
        let busy = self.tailscale_busy.is_some();
        let mut serve = row(
            "tailscale-serve",
            "Serve over Tailscale",
            &tailscale_state(tailscale),
            theme,
        );
        if local {
            let label = if busy {
                "Working…"
            } else if on {
                "Turn Off"
            } else {
                "Turn On"
            };
            serve = serve.child(
                button("tailscale-toggle", label, !on, theme)
                    .on_click(cx.listener(move |ws, _, _, cx| ws.set_tailscale(!on, cx))),
            );
        }
        rows.push(serve.into_any_element());
        if !local {
            rows.push(
                row(
                    "tailscale-remote",
                    "Managed on its own computer",
                    "The server only takes Tailscale changes over its local socket.",
                    theme,
                )
                .into_any_element(),
            );
        }
        if let Some(error) = &self.tailscale_error {
            rows.push(
                div()
                    .id("tailscale-error")
                    .px(px(12.))
                    .text_size(px(12.))
                    .text_color(theme.error)
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if state == "needs_login" && local {
            let link = tailscale["auth_url"].as_str().map(str::to_owned);
            let mut sign_in = row(
                "tailscale-sign-in",
                "Sign in to Tailscale",
                link.as_deref()
                    .unwrap_or("Ask Tailscale for a sign-in link."),
                theme,
            );
            sign_in = match link {
                Some(link) => sign_in.child(
                    button("tailscale-open-link", "Open Sign-in Link", true, theme)
                        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&link))),
                ),
                None => sign_in.child(button("tailscale-login", "Get Link", true, theme).on_click(
                    cx.listener(|ws, _, _, cx| {
                        ws.tailscale_call("server.tailscale.login", json!({}), cx)
                    }),
                )),
            };
            rows.push(sign_in.into_any_element());
        }
        let members = if on {
            tailscale["allow_tailnet_members"] == true
        } else {
            self.tailscale_members
        };
        rows.push(
            row(
                "tailscale-limit",
                "Limit access to your Tailscale account",
                "Off lets in anyone your tailnet's access rules allow.",
                theme,
            )
            .when(local, |r| {
                r.child(
                    button(
                        "tailscale-limit-toggle",
                        if members { "Off" } else { "On" },
                        !members,
                        theme,
                    )
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.set_tailscale_members(!members, cx)),
                    ),
                )
            })
            .into_any_element(),
        );
        if let Some(Overlay::Settings(page)) = &self.overlay
            && local
        {
            let mut hostname = row(
                "tailscale-hostname",
                "Preferred hostname",
                "The device name while serving; Tailscale may assign another.",
                theme,
            )
            .child(
                div()
                    .w(px(200.))
                    .flex_none()
                    .child(page.ts_hostname.clone()),
            );
            if state == "running" {
                hostname =
                    hostname.child(button("tailscale-rename", "Rename", false, theme).on_click(
                        cx.listener(|ws, _, _, cx| {
                            let (name, _, _) = ws.tailscale_fields(cx).unwrap_or_default();
                            ws.tailscale_call(
                                "server.tailscale.rename",
                                json!({"hostname": name}),
                                cx,
                            );
                        }),
                    ));
            }
            rows.push(hostname.into_any_element());
            // Tags and the control server only apply when the device signs in.
            let fixed = |id: &'static str, title: &str, value: Option<String>, empty: &str| {
                row(
                    id,
                    title,
                    &format!(
                        "{}. Sign out and back in to change it.",
                        value.unwrap_or_else(|| empty.to_owned())
                    ),
                    theme,
                )
                .into_any_element()
            };
            if on {
                let tags = tailscale["tags"].as_array().map(|t| {
                    t.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                rows.push(fixed("tailscale-tags", "Tags", tags, "None"));
                rows.push(fixed(
                    "tailscale-control",
                    "Control server",
                    tailscale["control_url"].as_str().map(str::to_owned),
                    "Tailscale's own",
                ));
            } else {
                rows.push(Self::field_row(
                    "tailscale-tags-field",
                    "Tags",
                    "Sign the device in with these tags, such as tag:rex.",
                    &page.ts_tags,
                    theme,
                ));
                rows.push(Self::field_row(
                    "tailscale-control-field",
                    "Control server",
                    "Sign in with another control server, such as Headscale.",
                    &page.ts_control,
                    theme,
                ));
            }
        }
        if on && local && state != "needs_login" {
            rows.push(
                row(
                    "tailscale-sign-out",
                    "Sign out",
                    "Sign the device out and turn off built-in Tailscale.",
                    theme,
                )
                .child(
                    button("tailscale-logout", "Sign Out", false, theme).on_click(cx.listener(
                        |ws, _, _, cx| ws.tailscale_call("server.tailscale.logout", json!({}), cx),
                    )),
                )
                .into_any_element(),
            );
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_listeners_and_tailscale_are_remote_addresses() {
        let status = json!({
            "listeners": [
                {"endpoint": "unix:///run/rex.sock", "addresses": ["/run/rex.sock"],
                 "transport": "unix", "admission": "peer_credentials", "primary": true},
                {"endpoint": "127.0.0.1:0", "addresses": ["127.0.0.1:44221"],
                 "transport": "tcp", "admission": "anonymous"},
                {"endpoint": "tailnet://:7297", "transport": "tailscale",
                 "admission": "tailscale_owner"}
            ],
            "tailscale": {"state": "running", "dns_name": "rex.tail1.ts.net",
                "allow_tailnet_members": true,
                "https": {"state": "available", "url": "https://rex.tail1.ts.net"}}
        });
        let found = remote_addresses(&status);
        let addresses: Vec<_> = found.iter().map(|r| r.address.as_str()).collect();
        assert_eq!(
            addresses,
            [
                "http://127.0.0.1:44221",
                "tailnet://:7297",
                "https://rex.tail1.ts.net"
            ]
        );
        assert_eq!(found[0].admits, "Anyone who can reach it, with full access");
        assert_eq!(found[2].admits, "Anyone your tailnet's access rules allow");
        assert!(remote_addresses(&json!({"listeners": [status["listeners"][0]]})).is_empty());
    }

    #[test]
    fn tailscale_states_read_as_words() {
        assert_eq!(tailscale_state(&json!({"state": "disabled"})), "Off");
        assert_eq!(
            tailscale_state(&json!({"state": "disabled", "error": "port in use"})),
            "Off: port in use"
        );
        assert_eq!(
            tailscale_state(&json!({"state": "needs_login", "auth_url": "https://login"})),
            "Waiting for you to sign in"
        );
        assert_eq!(
            tailscale_state(&json!({"state": "running", "dns_name": "rex.t.ts.net",
                "account": {"login_name": "me@example.com"},
                "https": {"state": "pending"}})),
            "On as rex.t.ts.net, signed in as me@example.com, waiting for its HTTPS certificate"
        );
        assert_eq!(
            tailscale_state(&json!({"state": "something new"})),
            "Starting…"
        );
    }
}
