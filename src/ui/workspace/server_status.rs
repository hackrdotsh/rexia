//! Settings › Server: whether the server is running and for how long, its version and API
//! details, and whether it and rexia fit together.
use super::{
    Workspace,
    preferences::{button, row},
};
use crate::{
    rex::client::{API_REVISION, MIN_API_REVISION},
    ui::theme::Theme,
};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use serde_json::{Value, json};

/// What the server said about itself, fetched when the Server section opens.
#[derive(Clone, Debug)]
pub struct ServerReport {
    /// The host it describes.
    pub host: usize,
    /// The server runs on this computer.
    pub local: bool,
    /// `server.status`, or why it couldn't be read.
    pub status: Result<Value, String>,
    /// `server.version`, or why it couldn't be read.
    pub version: Result<Value, String>,
    /// The version of the `rex` installed here, for a local server.
    pub installed: Option<String>,
    /// When the report was made, in Unix seconds.
    pub fetched: u64,
    /// The host's Tailscale event count when the report was made.
    pub tailscale_revision: u64,
}

impl Default for ServerReport {
    fn default() -> Self {
        Self {
            host: 0,
            local: false,
            status: Err("not asked yet".into()),
            version: Err("not asked yet".into()),
            installed: None,
            fetched: 0,
            tailscale_revision: 0,
        }
    }
}

/// The Server section's report: not asked for yet, on its way, or here.
#[derive(Clone, Debug, Default)]
pub enum ServerReportState {
    #[default]
    Missing,
    Loading,
    Ready(ServerReport),
}

/// One thing the compatibility check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// The server speaks the API revision rexia was built for.
    Compatible { revision: u64 },
    /// The server is older than rexia: what it lacks stays off.
    OlderServer { server: u64 },
    /// The server is newer than rexia: rexia uses what it knows.
    NewerServer { server: u64 },
    /// The rex installed here is another version than the server running: the server
    /// started before an update, and a restart runs the new one.
    Outdated { running: String, installed: String },
    /// Web client origins changed since rexia started the server, or the server running
    /// wasn't started by rexia, so they may not apply.
    PendingOrigins { started_by_rexia: bool },
    /// The key bindings rexia has came from an older configuration or server process.
    StaleKeymap,
}

impl Finding {
    /// Whether this is worth acting on.
    pub fn warns(&self) -> bool {
        !matches!(
            self,
            Finding::Compatible { .. } | Finding::NewerServer { .. }
        )
    }

    pub fn message(&self) -> String {
        match self {
            Finding::Compatible { revision } => {
                format!("Compatible: the server speaks API revision {revision}, as rexia does.")
            }
            Finding::OlderServer { server } => format!(
                "The server speaks API revision {server}, older than rexia's {API_REVISION}. \
                 rexia works, but what newer revisions add stays off until the server is updated."
            ),
            Finding::NewerServer { server } => format!(
                "The server speaks API revision {server}, newer than rexia's {API_REVISION}. \
                 rexia works with what it knows; update rexia for the rest."
            ),
            Finding::PendingOrigins {
                started_by_rexia: true,
            } => "Web client origins changed since rexia started the server. Restart the \
                 server to apply them."
                .into(),
            Finding::PendingOrigins {
                started_by_rexia: false,
            } => "This server wasn't started by rexia, so the web client origins in \
                 Settings › Developer may not apply. Restart the server to apply them."
                .into(),
            Finding::StaleKeymap => {
                "The key bindings rexia has are from an older configuration or server. \
                 Reload them to use the current ones."
                    .into()
            }
            Finding::Outdated { running, installed } => format!(
                "Rex {installed} is installed, but the server running is {running}. \
                 Restart the server to run the installed version."
            ),
        }
    }
}

/// Whether `keymap` came from the configuration and server process `status` describes.
pub fn keymap_current(keymap: &Value, status: &Value) -> bool {
    let config = &keymap["config"];
    config["hash"] == status["config"]["hash"] && config["instance_id"] == status["instance_id"]
}

/// Whether the origins rexia applied differ from those in Settings, or never reached the
/// server process running now. `None` when nothing is pending.
pub fn origins_pending(
    wanted: &[String],
    applied: Option<&crate::settings::AppliedOrigins>,
    instance: Option<&str>,
) -> Option<Finding> {
    let started_by_rexia =
        applied.is_some_and(|a| a.instance.as_deref() == instance && instance.is_some());
    let applied_origins = applied
        .filter(|_| started_by_rexia)
        .map_or(&[][..], |a| a.origins.as_slice());
    if wanted == applied_origins {
        return None;
    }
    Some(Finding::PendingOrigins { started_by_rexia })
}

/// What the report says about rexia and the server together.
pub fn findings(report: &ServerReport) -> Vec<Finding> {
    let mut found = Vec::new();
    let version = report.version.as_ref().ok();
    if let Some(server) = version.and_then(|v| v["api_revision"].as_u64()) {
        found.push(if server == u64::from(API_REVISION) {
            Finding::Compatible { revision: server }
        } else if server < u64::from(API_REVISION) {
            Finding::OlderServer { server }
        } else {
            Finding::NewerServer { server }
        });
    }
    let running = version.and_then(|v| v["rex_version"].as_str());
    if let (Some(running), Some(installed)) = (running, report.installed.as_deref())
        && running != installed
    {
        found.push(Finding::Outdated {
            running: running.to_owned(),
            installed: installed.to_owned(),
        });
    }
    found
}

/// Seconds since the Unix epoch for an RFC 3339 time such as `2026-11-14T18:02:18.92Z`.
pub fn unix_seconds(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut parts = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let clock_end = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(clock_end);
    let mut hms = clock.split(':');
    let hour: i64 = hms.next()?.parse().ok()?;
    let minute: i64 = hms.next()?.parse().ok()?;
    let second: i64 = hms.next()?.split('.').next()?.parse().ok()?;
    let offset = match zone.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let (h, m) = zone[1..].split_once(':')?;
            let minutes = h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?;
            if sign == '+' {
                minutes * 60
            } else {
                -minutes * 60
            }
        }
        _ => 0,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// A running time in words: "2 days 3 h", "4 h 12 min", "7 min", "less than a minute".
pub fn uptime(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => "less than a minute".into(),
        (0, 0, m) => format!("{m} min"),
        (0, h, m) => format!("{h} h {m} min"),
        (1, h, _) => format!("1 day {h} h"),
        (d, h, _) => format!("{d} days {h} h"),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The version `rex version` reports, from its first line (`rex 0.1.0-preview.19`).
fn installed_rex() -> Option<String> {
    let rex = std::env::var("REXIA_REX").unwrap_or_else(|_| "rex".into());
    let output = std::process::Command::new(rex)
        .arg("version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    Some(first.strip_prefix("rex ").unwrap_or(first).to_owned())
}

impl Workspace {
    /// Settings changes the server hasn't taken up: origins, and the key bindings.
    pub fn pending_findings(&self, report: &ServerReport) -> Vec<Finding> {
        let mut found = Vec::new();
        let Ok(status) = &report.status else {
            return found;
        };
        let applied = self.settings.applied_origins.get(&self.host().config.id);
        if report.local
            && let Some(pending) = origins_pending(
                &self.settings.web_origins,
                applied,
                status["instance_id"].as_str(),
            )
        {
            found.push(pending);
        }
        if let Some(keymap) = &self.model().keymap
            && !keymap_current(keymap, status)
        {
            found.push(Finding::StaleKeymap);
        }
        found
    }

    /// Once the server rexia restarted is running, note which process it is.
    fn note_server_instance(&mut self, report: &ServerReport, cx: &mut Context<Self>) {
        let Ok(status) = &report.status else { return };
        let Some(instance) = status["instance_id"].as_str().map(str::to_owned) else {
            return;
        };
        let host = self.host().config.id.clone();
        let Some(applied) = self.settings.applied_origins.get(&host) else {
            return;
        };
        if applied.instance.is_none() && applied.before.as_deref() != Some(instance.as_str()) {
            self.update_settings(
                move |s| {
                    if let Some(applied) = s.applied_origins.get_mut(&host) {
                        applied.instance = Some(instance);
                    }
                },
                cx,
            );
        }
    }

    /// Ask the server for the key bindings again.
    pub fn reload_keymap(&mut self, cx: &mut Context<Self>) {
        self.host()
            .handle
            .send(crate::rex::host::HostCommand::Keymap(
                crate::ui::actions::keymap_request(&self.settings.shortcuts),
            ));
        self.refresh_server_report(cx);
    }

    /// Ask the server about itself again, for the Server section.
    pub fn refresh_server_report(&mut self, cx: &mut Context<Self>) {
        let host = self.active;
        let handle = self.host().handle.clone();
        let local = handle.endpoint.is_local();
        let tailscale_revision = handle.snapshot().tailscale_revision;
        self.server_report = ServerReportState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let status = handle.request("server.status", json!({})).await;
            let version = handle.request("server.version", json!({})).await;
            let installed = if local {
                cx.background_executor()
                    .spawn(async { installed_rex() })
                    .await
            } else {
                None
            };
            let report = ServerReport {
                host,
                local,
                status: status.map_err(|e| e.to_string()),
                version: version.map_err(|e| e.to_string()),
                installed,
                fetched: now_unix(),
                tailscale_revision,
            };
            let _ = this.update(cx, |ws, cx| {
                ws.note_server_instance(&report, cx);
                ws.server_report = ServerReportState::Ready(report);
                cx.notify();
            });
        })
        .detach();
    }

    /// The report the Server section shows, once it has arrived.
    pub fn server_report(&self) -> Option<&ServerReport> {
        match &self.server_report {
            ServerReportState::Ready(report) => Some(report),
            _ => None,
        }
    }

    /// One line on whether the server runs and for how long.
    pub fn server_status_line(&self) -> String {
        let Some(report) = self.server_report() else {
            return "Asking the server…".into();
        };
        let status = match &report.status {
            Ok(status) => status,
            Err(error) => return format!("Not reachable: {error}"),
        };
        let place = if report.local {
            "on this computer"
        } else {
            "on a remote host"
        };
        let started = status["started_at"].as_str().and_then(unix_seconds);
        match started {
            Some(started) => format!(
                "Running {place} for {}",
                uptime(report.fetched.saturating_sub(started.max(0) as u64))
            ),
            None => format!("Running {place}"),
        }
    }

    pub(super) fn server_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let report = self.server_report();
        let detail = |label: &str, value: String| {
            div()
                .flex()
                .gap(px(12.))
                .text_size(px(12.))
                .child(
                    div()
                        .w(px(150.))
                        .flex_none()
                        .text_color(theme.text_muted)
                        .child(label.to_owned()),
                )
                .child(div().flex_1().min_w_0().child(value))
                .into_any_element()
        };
        let mut rows = vec![
            row(
                "server-status",
                "Server status",
                &self.server_status_line(),
                theme,
            )
            .child(
                button("server-refresh", "Refresh", false, theme)
                    .on_click(cx.listener(|ws, _, _, cx| ws.refresh_server_report(cx))),
            )
            .into_any_element(),
        ];
        if let Some(report) = report {
            let version = report.version.as_ref().ok();
            let status = report.status.as_ref().ok();
            let text = |value: Option<&Value>, key: &str| {
                value
                    .and_then(|v| v[key].as_str())
                    .unwrap_or("unknown")
                    .to_owned()
            };
            let number = |value: Option<&Value>, key: &str| {
                value
                    .and_then(|v| v[key].as_u64())
                    .map_or("unknown".to_owned(), |n| n.to_string())
            };
            let commit = text(version, "commit");
            let mut details = vec![
                detail("Version", text(version, "rex_version")),
                detail(
                    "Built",
                    format!(
                        "{} on {}{}",
                        commit.get(..12).unwrap_or(&commit),
                        text(version, "built_at"),
                        if version.is_some_and(|v| v["dirty"] == true) {
                            ", with local changes"
                        } else {
                            ""
                        }
                    ),
                ),
                detail("Platform", text(version, "platform")),
                detail(
                    "API revision",
                    format!(
                        "{} (supports {} to {}); rexia speaks {MIN_API_REVISION} to {API_REVISION}",
                        number(version, "api_revision"),
                        version
                            .and_then(|v| v["api_min_revision"].as_u64())
                            .unwrap_or(0),
                        number(version, "api_revision"),
                    ),
                ),
                detail("Process", number(status, "pid")),
                detail("Sessions", number(status, "session_count")),
                detail("Instance", text(status, "instance_id")),
            ];
            if let Some(installed) = &report.installed {
                details.push(detail("Installed rex", installed.clone()));
            }
            rows.push(
                div()
                    .id("server-details")
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .py(px(8.))
                    .px(px(12.))
                    .children(details)
                    .into_any_element(),
            );
            let mut found = findings(report);
            found.extend(self.pending_findings(report));
            let outdated = found
                .iter()
                .any(|f| matches!(f, Finding::Outdated { .. } | Finding::PendingOrigins { .. }));
            let stale = found.contains(&Finding::StaleKeymap);
            let lines = found.iter().map(|finding| {
                div()
                    .text_size(px(12.))
                    .text_color(if finding.warns() {
                        theme.error
                    } else {
                        theme.text_muted
                    })
                    .child(finding.message())
            });
            rows.push(
                row(
                    "server-compatibility",
                    "Compatibility",
                    "How this server and rexia fit together.",
                    theme,
                )
                .when(stale, |r| {
                    r.child(
                        button(
                            "server-reload-keys",
                            "Reload Key Bindings",
                            !outdated,
                            theme,
                        )
                        .on_click(cx.listener(|ws, _, _, cx| ws.reload_keymap(cx))),
                    )
                })
                .when(outdated && report.local, |r| {
                    r.child(
                        button("server-restart", "Restart Server", true, theme).on_click(
                            cx.listener(|ws, _, _, cx| {
                                let _ = ws.ask_restart_server(cx);
                            }),
                        ),
                    )
                })
                .into_any_element(),
            );
            rows.push(
                div()
                    .id("server-findings")
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(12.))
                    .children(lines)
                    .into_any_element(),
            );
        }
        rows.extend(self.remote_section(theme, cx));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_with_fractions_and_offsets() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            unix_seconds("2026-11-14T18:02:18.922364418Z"),
            Some(1_794_679_338)
        );
        assert_eq!(
            unix_seconds("2026-11-14T19:02:18+01:00"),
            unix_seconds("2026-11-14T18:02:18Z")
        );
        assert_eq!(unix_seconds("yesterday"), None);
    }

    #[test]
    fn uptimes_read_naturally() {
        assert_eq!(uptime(30), "less than a minute");
        assert_eq!(uptime(7 * 60), "7 min");
        assert_eq!(uptime(4 * 3600 + 12 * 60), "4 h 12 min");
        assert_eq!(uptime(86_400 + 3 * 3600), "1 day 3 h");
        assert_eq!(uptime(5 * 86_400), "5 days 0 h");
    }

    #[test]
    fn pending_settings_show_until_the_server_takes_them() {
        use crate::settings::AppliedOrigins;
        let status = json!({"instance_id": "b", "config": {"hash": "h2"}});
        // Key bindings from the same file and process are current; others are stale.
        assert!(keymap_current(
            &json!({"config": {"hash": "h2", "instance_id": "b"}}),
            &status
        ));
        assert!(!keymap_current(
            &json!({"config": {"hash": "h1", "instance_id": "b"}}),
            &status
        ));
        assert!(!keymap_current(
            &json!({"config": {"hash": "h2", "instance_id": "a"}}),
            &status
        ));
        // No origins wanted, none applied: nothing pending.
        assert_eq!(origins_pending(&[], None, Some("b")), None);
        let wanted = vec!["https://*.a.dev".to_owned()];
        // Wanted, but this server wasn't started by rexia.
        assert_eq!(
            origins_pending(&wanted, None, Some("b")),
            Some(Finding::PendingOrigins {
                started_by_rexia: false
            })
        );
        let applied = AppliedOrigins {
            origins: wanted.clone(),
            before: Some("a".into()),
            instance: Some("b".into()),
        };
        assert_eq!(origins_pending(&wanted, Some(&applied), Some("b")), None);
        // Changed since, or the server restarted some other way.
        assert_eq!(
            origins_pending(&[], Some(&applied), Some("b")),
            Some(Finding::PendingOrigins {
                started_by_rexia: true
            })
        );
        assert_eq!(
            origins_pending(&wanted, Some(&applied), Some("c")),
            Some(Finding::PendingOrigins {
                started_by_rexia: false
            })
        );
        assert!(
            Finding::StaleKeymap.warns()
                && Finding::PendingOrigins {
                    started_by_rexia: true
                }
                .warns()
        );
    }

    #[test]
    fn findings_compare_revisions_and_versions() {
        let report = |api: u32, running: &str, installed: Option<&str>| ServerReport {
            version: Ok(json!({"api_revision": api, "rex_version": running})),
            installed: installed.map(str::to_owned),
            ..Default::default()
        };
        assert_eq!(
            findings(&report(
                API_REVISION,
                "0.1.0-preview.19",
                Some("0.1.0-preview.19")
            )),
            vec![Finding::Compatible {
                revision: u64::from(API_REVISION)
            }]
        );
        let older = findings(&report(12, "0.1.0-preview.17", Some("0.1.0-preview.19")));
        assert_eq!(
            older,
            vec![
                Finding::OlderServer { server: 12 },
                Finding::Outdated {
                    running: "0.1.0-preview.17".into(),
                    installed: "0.1.0-preview.19".into()
                }
            ]
        );
        assert!(older.iter().all(Finding::warns));
        assert_eq!(
            findings(&report(API_REVISION + 1, "x", None)),
            vec![Finding::NewerServer {
                server: u64::from(API_REVISION) + 1
            }]
        );
    }
}
