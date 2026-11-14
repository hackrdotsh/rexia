//! Terminal › Keyboard and Integrated Path Browser against a real Rex server named by
//! REXIA_TEST_SERVER (its init.lua binds ctrl+a>c): key sequence hints on and off, and
//! recently chosen paths first in the path browser.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::settings::Settings;
use serde_json::Value;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

#[test]
fn key_hints_and_recent_paths() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-paths-keys");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt and the keymap", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
    });

    // ctrl+a starts a sequence: its next keys show, unless hints are off.
    h.press("ctrl-a");
    let on = h.workspace(|ws| ws.showing_key_hints());
    h.press("escape");
    settings(&mut h, |s| s.terminal.key_hints = false);
    h.press("ctrl-a");
    let off = h.workspace(|ws| ws.showing_key_hints());
    h.press("escape");
    println!("hints after ctrl+a: on {on}, off {off}");
    assert!(on && !off);

    // Go to Directory /tmp: it's remembered, and listed first next time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    h.run("pane.go_to_directory", Value::Null);
    h.type_text("/tmp");
    h.press("enter");
    // It's remembered once the pane has gone there.
    let recent = h.wait_ui("the recent path", |ws| {
        let recent = ws.settings.recent_paths.clone();
        recent
            .values()
            .any(|paths| paths.first().map(String::as_str) == Some("/tmp/"))
            .then_some(recent)
    });
    println!("recent paths: {recent:?}");
    h.run("pane.go_to_directory", Value::Null);
    let titles = loop {
        h.pump();
        let titles = h.workspace(|ws| ws.list_titles());
        if titles.len() > 1 {
            break titles;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the directory list never filled"
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    };
    println!(
        "path browser lists first: {:?}",
        &titles[..3.min(titles.len())]
    );
    assert_eq!(titles[0], "/tmp/");
    h.screenshot("recent-paths");
    h.press("escape");

    // With recent paths off, the list starts with the directory's own entries.
    settings(&mut h, |s| s.terminal.recent_paths = false);
    h.run("pane.go_to_directory", Value::Null);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let titles = loop {
        h.pump();
        let titles = h.workspace(|ws| ws.list_titles());
        if !titles.is_empty() {
            break titles;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the directory list never filled"
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    };
    println!(
        "with recent paths off: {:?}",
        &titles[..3.min(titles.len())]
    );
    assert_ne!(titles[0], "/tmp/");
    h.press("escape");
    drop(sessions);
}
