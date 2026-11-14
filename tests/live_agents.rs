//! Settings › Integrations: installing and removing Rex's skill and hooks for coding
//! agents with REXIA_TEST_REX's `rex skill`, in a throwaway home with Claude Code and Codex
//! "installed". Never touches the real home. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{SettingsSection, agents::AgentsReport};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Wait for `rex skill` to finish and report; the report then.
fn settled(h: &mut Harness) -> AgentsReport {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        h.pump();
        let state = h.workspace(|ws| ws.agents_state().clone());
        if !state.busy
            && let Some(report) = state.report
        {
            return report.expect("rex skill status");
        }
        assert!(Instant::now() < deadline, "rex skill didn't finish");
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn update(
    h: &mut Harness,
    change: impl FnOnce(
        &mut rexia::ui::workspace::Workspace,
        &mut gpui::Context<rexia::ui::workspace::Workspace>,
    ),
) {
    let window = h.window;
    h.cx.update(|cx| window.update(cx, |ws, _, cx| change(ws, cx)).unwrap());
    h.pump();
}

#[test]
fn coding_agents_get_rex_and_lose_it_again() {
    let (Some(address), Ok(rex)) = (common::server(), std::env::var("REXIA_TEST_REX")) else {
        eprintln!("skipped: needs REXIA_TEST_SERVER and REXIA_TEST_REX");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-agents");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe {
        std::env::set_var("REXIA_REX", &rex);
        std::env::set_var("HOME", home.path());
        std::env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var("CODEX_HOME");
    }

    h.run("client.settings", json!({}));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.show_settings_section(SettingsSection::Integrations, window, cx)
            })
            .unwrap()
    });
    let before = settled(&mut h);
    let found: Vec<_> = before
        .agents
        .iter()
        .map(|a| (a.id.as_str(), a.detected))
        .collect();
    println!("agents: {found:?}, integrated {}", before.integrated());
    assert!(!before.integrated());
    assert!(found.contains(&("claude", true)) && found.contains(&("codex", true)));

    // Integrate: the skill and hooks go in, naming off.
    update(&mut h, |ws, cx| ws.setup_agents(None, cx));
    let installed = settled(&mut h);
    let message = h.workspace(|ws| ws.agents_state().message.clone());
    println!("installed: {message:?}");
    assert!(installed.integrated() && !installed.session_naming());
    assert!(home.path().join(".claude/skills/rex/SKILL.md").exists());
    assert!(home.path().join(".codex/hooks.json").exists());
    for agent in installed.agents.iter().filter(|a| a.detected) {
        println!("{}: {}", agent.title(), agent.summary());
    }

    // Let agents name sessions.
    update(&mut h, |ws, cx| ws.setup_agents(Some(true), cx));
    let naming = settled(&mut h);
    println!("naming: {}", naming.session_naming());
    assert!(naming.session_naming());
    h.screenshot("settings-integrations");

    // Install Rex for Agents from the palette, outside Settings: a notice says what it did.
    h.press("escape");
    h.run("client.agents.setup", Value::Null);
    settled(&mut h);
    let notice = h.workspace(|ws| ws.notice.clone());
    println!("notice: {notice:?}");
    assert!(notice.is_some_and(|n| n.starts_with("Rex for agents:")));

    // Remove: Rex's files go, and nothing else in the home.
    update(&mut h, |ws, cx| ws.remove_agents(cx));
    let removed = settled(&mut h);
    println!("removed: integrated {}", removed.integrated());
    assert!(!removed.integrated());
    assert!(!home.path().join(".claude/skills/rex").exists());
    assert!(home.path().join(".claude").exists());
    drop(sessions);
}
