//! Program activity: badges on tabs, desktop notifications for panes out of view, clicks that
//! show the pane, and the sidebar's activity list. Runs against a real Rex server named by
//! REXIA_TEST_SERVER and a private session bus, never the desktop's.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use rexia::rex::host::HostCommand;
use serde_json::json;

fn paste_and_run(h: &mut Harness, session: &str, block: &str, line: &str) {
    for (method, payload) in [
        (
            "block.event",
            json!({"session_id": session, "block_id": block,
                "event": {"type": "paste", "payload": {"text": line}}}),
        ),
        (
            "client.key",
            json!({"session_id": session, "block_id": block,
                "key": {"key": "enter", "mods": "", "text": "\r", "unshifted_codepoint": 13, "action": "press"}}),
        ),
    ] {
        h.host.send(HostCommand::Input {
            session_id: session.into(),
            block_id: block.into(),
            method,
            payload,
            claim: true,
        });
    }
    h.pump();
}

#[test]
fn background_attention_notifies_badges_and_clicks_through() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-notify");
    let session = sessions.ids[0].clone();
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.toml");
    let mut h = Harness::open_with_path(
        common::test_settings(&address, &session),
        Some(settings_path.clone()),
        1280.,
        800.,
    );
    h.attach_desktop(&desktop.address);
    h.activate();
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let focused = h.cx.update(|cx| {
        let window = h.window;
        window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap()
    });
    println!("window active after activate: {focused}");

    // A second tab, in the background, whose program comes to need the user.
    let created = h
        .call_from(
            &control,
            "session.new_window",
            json!({"session_id": session, "window_label": "agent",
                "layout": rexia::rex::host::shell_layout(), "focus": false}),
        )
        .unwrap();
    let background = created["block_ids"][0]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            h.wait("the background tab's pane", |m| {
                let view = m.view()?;
                let window = view.windows.iter().find(|w| w.label == "agent")?;
                window.blocks().next()?.block_id.clone()
            })
        });
    h.wait("the background shell", |m| {
        m.blocks
            .get(&background)
            .and_then(|b| b.process.name.clone())
    });
    // Status records belong to the reporting process, so it stays alive.
    paste_and_run(
        &mut h,
        &session,
        &background,
        "sh -c 'printf \"\\033]7501;state=blocked:kind=question:app=claude:msg=QWxsb3cgZWRpdD8=\\033\\\\\\\\\"; sleep 120'",
    );

    let note = desktop.wait_shown(
        "the blocked notification",
        || h.pump(),
        |s| s.summary.contains("claude"),
    );
    println!("notification: {note:?}");
    assert_eq!(note.summary, "claude: Question");
    assert!(note.body.contains("Allow edit?"), "body: {}", note.body);
    assert_eq!(note.urgency, 2, "attention is critical");

    let marks = h.wait_ui("the tab's badge", |ws| {
        let marks = ws.tab_marks();
        marks.iter().any(Option::is_some).then_some(marks)
    });
    println!("tab marks: {marks:?}");
    assert_eq!(marks, vec![None, Some("Question")]);
    assert_eq!(h.workspace(|ws| ws.unseen_attention()), 1);

    // The combined layout puts Activity below Sessions and persists the choice.
    assert!(h.workspace(|ws| ws.element_bounds("activity-below-sessions").is_none()));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.interface.activity_below_sessions = true, cx);
            })
            .unwrap();
    });
    let tab_id = h
        .host
        .snapshot()
        .view()
        .unwrap()
        .active_window()
        .unwrap()
        .id()
        .to_owned();
    let (tab, activity) = h.wait_ui("Sessions with Activity below", |ws| {
        Some((
            ws.element_bounds(&format!("side-tab:{tab_id}"))?,
            ws.element_bounds("activity-below-sessions")?,
        ))
    });
    assert!(activity.origin.y >= tab.origin.y + tab.size.height);
    assert!(
        rexia::settings::Settings::load(&settings_path)
            .unwrap()
            .interface
            .activity_below_sessions
    );
    h.screenshot("notify-activity-below-sessions");

    // The standalone Activity view still works.
    h.run("client.sidebar.activity.toggle", json!({}));
    assert!(h.workspace(|ws| ws.settings.interface.activity_view));
    h.screenshot("notify-activity");
    assert!(h.workspace(|ws| ws.element_bounds("activity-below-sessions").is_none()));

    // Clicking the notification shows its tab, which counts as seeing it.
    desktop.click(note.id);
    h.wait("the agent tab to show", |m| {
        let view = m.view()?;
        (view.active_window()?.label == "agent").then_some(())
    });
    let closed = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            h.pump();
            let closed = desktop.closed();
            if !closed.is_empty() || std::time::Instant::now() > deadline {
                break closed;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    };
    println!("closed after click: {closed:?}");
    assert_eq!(closed, vec![note.id], "seeing the pane withdraws its note");
    assert_eq!(h.workspace(|ws| ws.unseen_attention()), 0);
    // Blocked stays marked while it holds; only Done clears once seen.
    assert_eq!(
        h.workspace(|ws| ws.tab_marks()),
        vec![None, Some("Question")]
    );
    h.screenshot("notify-clicked");

    // A program's own notification (OSC 9) from a tab out of view.
    h.call_from(
        &control,
        "session.focus_window",
        json!({"session_id": session, "window_id": h.host.snapshot().view().unwrap().windows[0].id()}),
    )
    .unwrap();
    h.wait("the first tab to show", |m| {
        (m.view()?.active_window()?.label != "agent").then_some(())
    });
    // A program's own notification from a third tab, also out of view.
    let created = h
        .call_from(
            &control,
            "session.new_window",
            json!({"session_id": session, "window_label": "build",
                "layout": rexia::rex::host::shell_layout(), "focus": false}),
        )
        .unwrap();
    let builder = created["block_ids"][0]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            h.wait("the build tab's pane", |m| {
                let view = m.view()?;
                let window = view.windows.iter().find(|w| w.label == "build")?;
                window.blocks().next()?.block_id.clone()
            })
        });
    h.wait("the build shell", |m| {
        m.blocks.get(&builder).and_then(|b| b.process.name.clone())
    });
    paste_and_run(
        &mut h,
        &session,
        &builder,
        "printf '\\033]9;build finished\\007'",
    );
    let osc = desktop.wait_shown(
        "the OSC 9 notification",
        || h.pump(),
        |s| s.body == "build finished",
    );
    println!("osc 9 notification: {osc:?}");

    drop(sessions);
}
