//! Settings › Integrations: Rex's skill and session hooks for coding agents (Claude Code,
//! Codex, OpenCode, Pi), installed and removed by `rex skill`, as Rex for Mac does.
use super::{
    Workspace,
    preferences::{button, row},
};
use crate::ui::theme::Theme;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use serde::Deserialize;

/// One thing Rex installs into an agent: its skill or its hook.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AgentItem {
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    /// current, outdated, missing, foreign, unreadable or unsupported.
    pub state: String,
}

/// An agent Rex knows, as `rex skill status --json` reports it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub detected: bool,
    #[serde(default)]
    pub session_naming: Option<bool>,
    pub items: Vec<AgentItem>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct AgentsReport {
    #[serde(rename = "harnesses", default)]
    pub agents: Vec<Agent>,
}

impl Agent {
    pub fn title(&self) -> &str {
        match self.id.as_str() {
            "claude" => "Claude Code",
            "codex" => "Codex",
            "opencode" => "OpenCode",
            "pi" => "Pi",
            other => other,
        }
    }

    fn item(&self, kind: &str) -> Option<&AgentItem> {
        self.items.iter().find(|i| i.kind == kind)
    }

    /// Rex's skill is in this agent, current or from an older Rex.
    pub fn installed(&self) -> bool {
        self.item("skill")
            .is_some_and(|i| matches!(i.state.as_str(), "current" | "outdated"))
    }

    /// What's installed, in words.
    pub fn summary(&self) -> String {
        let state = |kind: &str| self.item(kind).map_or("unknown", |i| i.state.as_str());
        let words = |state: &str| {
            match state {
                "current" => "installed",
                "outdated" => "from an older Rex",
                "missing" => "not installed",
                "foreign" => "another skill named rex is there",
                "unreadable" => "can't be read",
                "unsupported" => "not supported",
                other => other,
            }
            .to_owned()
        };
        let found = if self.detected {
            "Found"
        } else {
            "Not found on this computer"
        };
        format!(
            "{found} · skill {} · hook {}",
            words(state("skill")),
            words(state("hook"))
        )
    }
}

impl AgentsReport {
    /// Integrated: Rex's skill is in at least one agent found here.
    pub fn integrated(&self) -> bool {
        self.agents.iter().any(|a| a.detected && a.installed())
    }

    /// Agents name sessions after their task, as the installed skill says.
    pub fn session_naming(&self) -> bool {
        self.agents
            .iter()
            .filter(|a| a.installed())
            .any(|a| a.session_naming == Some(true))
    }

    /// Something needs setup run again: an older Rex's files, or an agent found since.
    pub fn needs_update(&self) -> bool {
        self.integrated()
            && self.agents.iter().any(|a| {
                a.detected
                    && a.items
                        .iter()
                        .any(|i| matches!(i.state.as_str(), "outdated" | "missing"))
            })
    }
}

/// The state of the Integrations section: the last report, a run under way, and what
/// the last run said.
#[derive(Clone, Debug, Default)]
pub struct AgentsState {
    pub report: Option<Result<AgentsReport, String>>,
    pub busy: bool,
    pub message: Option<String>,
}

/// Run `rex skill ARGS --json` on this computer (`$REXIA_REX` names another rex).
fn rex_skill(args: &[String]) -> Result<serde_json::Value, String> {
    let rex = crate::rex::install::program();
    let output = std::process::Command::new(&rex)
        .arg("skill")
        .args(args)
        .arg("--json")
        .output()
        .map_err(|e| format!("Couldn't run {rex}: {e}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let line = said.lines().rev().find(|l| !l.trim().is_empty());
        return Err(line.unwrap_or("rex skill failed").trim().to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("rex skill said something unexpected: {e}"))
}

/// What a setup or removal changed, in words.
fn describe_steps(result: &serde_json::Value, removing: bool) -> String {
    let steps = result["steps"].as_array().cloned().unwrap_or_default();
    let changed: Vec<String> = steps
        .iter()
        .filter(|s| !matches!(s["change"].as_str(), Some("unchanged") | None))
        .map(|s| {
            let agents = s["harnesses"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|h| h.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!(
                "{} {} for {agents}",
                s["kind"].as_str().unwrap_or("item"),
                s["change"].as_str().unwrap_or("changed")
            )
        })
        .collect();
    let notes: Vec<String> = steps
        .iter()
        .filter_map(|s| s["note"].as_str().or(s["message"].as_str()))
        .map(str::to_owned)
        .collect();
    let mut text = if changed.is_empty() {
        if removing {
            "Nothing of Rex's was installed.".to_owned()
        } else {
            "Everything was already up to date.".to_owned()
        }
    } else {
        format!("{}.", changed.join("; "))
    };
    for note in notes {
        text.push(' ');
        text.push_str(&note);
    }
    text
}

impl Workspace {
    /// Ask `rex skill status` what's installed.
    pub fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        self.agents.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async {
                    rex_skill(&["status".into()]).and_then(|v| {
                        serde_json::from_value::<AgentsReport>(v)
                            .map_err(|e| format!("rex skill status said something unexpected: {e}"))
                    })
                })
                .await;
            let _ = this.update(cx, |ws, cx| {
                ws.agents.report = Some(report);
                ws.agents.busy = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Install Rex's skill and hooks into the agents found (`client.agents.setup`),
    /// telling them to name sessions or not, or keeping the installed choice with `None`.
    pub fn setup_agents(&mut self, naming: Option<bool>, cx: &mut Context<Self>) {
        let mut args = vec!["setup".to_owned()];
        if let Some(naming) = naming {
            args.push(format!("--session-naming={naming}"));
        }
        self.run_rex_skill(args, false, cx);
    }

    /// Remove Rex's skill and hooks from every agent.
    pub fn remove_agents(&mut self, cx: &mut Context<Self>) {
        self.run_rex_skill(vec!["remove".into()], true, cx);
    }

    fn run_rex_skill(&mut self, args: Vec<String>, removing: bool, cx: &mut Context<Self>) {
        self.agents.busy = true;
        self.agents.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { rex_skill(&args) })
                .await;
            let _ = this.update(cx, |ws, cx| {
                let message = match &result {
                    Ok(value) => describe_steps(value, removing),
                    Err(error) => format!("Couldn't change the agents: {error}"),
                };
                ws.agents.message = Some(message.clone());
                // Outside Settings, the action's result shows as a notice.
                if ws.settings_section().is_none() {
                    ws.notice = Some(if removing {
                        message
                    } else {
                        format!("Rex for agents: {message}")
                    });
                }
                ws.refresh_agents(cx);
            });
        })
        .detach();
    }

    /// The Integrations section's state, for tests.
    pub fn agents_state(&self) -> &AgentsState {
        &self.agents
    }

    pub(super) fn integrations_section(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let state = &self.agents;
        let report = state.report.as_ref();
        let heading = |text: &str| {
            div()
                .pt(px(8.))
                .pb(px(4.))
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(text.to_owned())
                .into_any_element()
        };
        let mut rows = vec![heading("Coding Agents")];
        let report = match report {
            None => {
                rows.push(
                    row(
                        "agents-loading",
                        "Coding agents",
                        "Asking rex skill status…",
                        theme,
                    )
                    .into_any_element(),
                );
                return rows;
            }
            Some(Err(error)) => {
                rows.push(
                    row(
                        "agents-error",
                        "Coding agents",
                        &format!("Couldn't read what's installed: {error}"),
                        theme,
                    )
                    .into_any_element(),
                );
                return rows;
            }
            Some(Ok(report)) => report,
        };
        let integrated = report.integrated();
        let label = match (state.busy, integrated) {
            (true, _) => "Working…",
            (false, true) => "Remove",
            (false, false) => "Install",
        };
        rows.push(
            row(
                "agents-integrate",
                "Integrate with available agents",
                "Install a Rex skill and session hooks for the agents found on this computer.",
                theme,
            )
            .child(
                button("agents-integrate-toggle", label, !integrated, theme).on_click(cx.listener(
                    move |ws, _, _, cx| {
                        if integrated {
                            ws.remove_agents(cx)
                        } else {
                            ws.setup_agents(None, cx)
                        }
                    },
                )),
            )
            .into_any_element(),
        );
        if report.needs_update() {
            rows.push(
                row(
                    "agents-update",
                    "Update",
                    "An agent has Rex's files from an older Rex, or was installed since.",
                    theme,
                )
                .child(
                    button("agents-update-run", "Update", true, theme)
                        .on_click(cx.listener(|ws, _, _, cx| ws.setup_agents(None, cx))),
                )
                .into_any_element(),
            );
        }
        for agent in &report.agents {
            rows.push(
                row(
                    format!("agent-{}", agent.id),
                    agent.title(),
                    &agent.summary(),
                    theme,
                )
                .into_any_element(),
            );
        }
        if let Some(message) = &state.message {
            rows.push(
                div()
                    .id("agents-message")
                    .px(px(12.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(message.clone())
                    .into_any_element(),
            );
        }
        if integrated {
            let naming = report.session_naming();
            rows.push(heading("Agent Behavior"));
            rows.push(
                row(
                    "agents-naming",
                    "Let agents name sessions",
                    "Agents label the session after the task they're working on.",
                    theme,
                )
                .child(
                    button(
                        "agents-naming-toggle",
                        if naming { "On" } else { "Off" },
                        naming,
                        theme,
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| ws.setup_agents(Some(!naming), cx))),
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
    use serde_json::json;

    fn report(skill: &str, naming: Option<bool>) -> AgentsReport {
        serde_json::from_value(json!({"scope": "user", "harnesses": [
            {"id": "claude", "detected": true, "session_naming": naming, "items": [
                {"kind": "skill", "path": "/h/.claude/skills/rex", "state": skill},
                {"kind": "hook", "path": "/h/.claude/settings.json", "state": skill}]},
            {"id": "pi", "detected": false, "items": [
                {"kind": "skill", "state": "missing"}, {"kind": "hook", "state": "unsupported"}]}
        ]}))
        .unwrap()
    }

    #[test]
    fn reports_read_as_installed_naming_and_updates() {
        let missing = report("missing", None);
        assert!(!missing.integrated());
        assert_eq!(
            missing.agents[0].summary(),
            "Found · skill not installed · hook not installed"
        );
        assert_eq!(missing.agents[1].title(), "Pi");
        let current = report("current", Some(true));
        assert!(current.integrated() && current.session_naming() && !current.needs_update());
        let old = report("outdated", Some(false));
        assert!(old.integrated() && !old.session_naming() && old.needs_update());
    }

    #[test]
    fn steps_read_as_words() {
        let result = json!({"steps": [
            {"harnesses": ["claude"], "kind": "skill", "change": "created"},
            {"harnesses": ["codex"], "kind": "hook", "change": "unchanged"}]});
        assert_eq!(describe_steps(&result, false), "skill created for claude.");
        assert_eq!(
            describe_steps(&json!({"steps": []}), true),
            "Nothing of Rex's was installed."
        );
    }
}
