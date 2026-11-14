//! The app-level actions, against a real Rex server named by REXIA_TEST_SERVER: Toggle
//! Visibility from another client, Send Key to Server from the palette, Change Shortcut,
//! and (with REXIA_TEST_REX) Restart Server, which restarts the test server itself.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{ListKind, Target};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    os::unix::fs::PermissionsExt,
    rc::Rc,
    time::{Duration, Instant},
};

fn window_active(h: &mut Harness) -> bool {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap()
    })
}

#[test]
fn visibility_key_forward_shortcuts_and_restart() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-app-actions");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let client_id = h.host.snapshot().client_id.unwrap();
    let hidden = Rc::new(Cell::new(0));
    let counter = hidden.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_window_hider(move |_| counter.set(counter.get() + 1))
            })
            .unwrap()
    });

    // Toggle Visibility, as a compositor's `rex do` would send it: in front, then hidden.
    assert!(!window_active(&mut h));
    let shown = h
        .call_from(
            &control,
            "client.perform",
            json!({"client_id": client_id, "action": "client.visibility.toggle"}),
        )
        .unwrap();
    h.pump();
    assert!(window_active(&mut h), "brought to the front");
    let hid = h
        .call_from(
            &control,
            "client.perform",
            json!({"client_id": client_id, "action": "client.visibility.toggle"}),
        )
        .unwrap();
    println!("toggle visibility: {shown} then {hid}");
    assert_eq!(hidden.get(), 1, "hidden once it was in front");

    // Send Key to Server is listed, and from the palette it explains it needs a key.
    h.run("client.palette.toggle", Value::Null);
    h.type_text("send key to server");
    let listed = h.workspace(|ws| ws.list_titles().first().cloned());
    assert_eq!(listed.as_deref(), Some("Send Key to Server"));
    h.press("escape");
    let refused = h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.run_action(
                    "client.key.forward",
                    Value::Null,
                    "palette",
                    Target::default(),
                    window,
                    cx,
                )
            })
            .unwrap()
    });
    println!("send key to server from the palette: {refused:?}");
    assert!(refused.is_err());

    // Change Shortcut: Split Pane Right moves to ctrl+alt+k, and that key splits.
    h.run("client.shortcuts.edit", Value::Null);
    assert_eq!(h.workspace(|ws| ws.list_kind()), Some(ListKind::Shortcuts));
    h.type_text("split pane right");
    let first = h.workspace(|ws| ws.list_titles().first().cloned());
    assert_eq!(first.as_deref(), Some("Split Pane Right"));
    h.press("enter");
    assert_eq!(
        h.workspace(|ws| ws.recording_shortcut()).as_deref(),
        Some("pane.split.right")
    );
    h.screenshot("change-shortcut");
    h.press("ctrl-alt-k");
    let saved = h.workspace(|ws| ws.settings.shortcuts.get("pane.split.right").cloned());
    println!("saved shortcut: {saved:?}");
    assert_eq!(saved, Some(vec!["ctrl+alt+k".to_owned()]));
    // The server answers with the new keymap; the key then splits the pane.
    h.wait_ui("the new binding", |ws| {
        ws.keys_for_action("pane.split.right")
            .filter(|k| k.contains("ctrl+alt+k"))
    });
    h.press("ctrl-alt-k");
    h.wait("the split", |m| {
        (m.view()?.active_window()?.panes().len() == 2).then_some(())
    });
    println!("ctrl+alt+k split the pane");

    // Restart Server: asked first, then the server stops and starts, and rexia reconnects.
    let Ok(real_rex) = std::env::var("REXIA_TEST_REX") else {
        eprintln!("skipped restart: set REXIA_TEST_REX");
        return;
    };
    let config = common::request(&control, "client.keymap", json!({})).unwrap()["config"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let before =
        common::request(&control, "server.status", json!({})).unwrap()["instance_id"].clone();
    let dir = tempfile::tempdir().unwrap();
    // Beside the test server's socket, which outlives this test, as the server does.
    let socket = address
        .strip_prefix("unix://")
        .expect("a unix:// test server");
    let state = std::path::Path::new(socket)
        .parent()
        .unwrap()
        .join("restart-state");
    std::fs::create_dir_all(&state).unwrap();
    let log = dir.path().join("rex.log");
    let fake = dir.path().join("rex");
    std::fs::write(
        &fake,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
endpoint=$2
setsid '{real_rex}' server run --listen "$endpoint" --state-dir '{state}' --config '{config}' --restore=false > '{state}/server.log' 2>&1 < /dev/null &
i=0
until '{real_rex}' -S "$endpoint" --autostart=false server status > /dev/null 2>&1 || [ $i -ge 100 ]; do
  sleep 0.1; i=$((i+1))
done
"#,
            log = log.display(),
            state = state.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_REX", &fake) };
    h.run("client.server.restart", Value::Null);
    assert!(h.workspace(|ws| ws.has_dialog()), "asked first");
    h.screenshot("restart-server");
    h.press("tab");
    h.press("enter");
    let deadline = Instant::now() + Duration::from_secs(30);
    let after = loop {
        h.pump();
        let model = h.host.snapshot();
        let id = model.server["instance_id"].clone();
        if model.connection == rexia::rex::host::Connection::Connected && id != before {
            break id;
        }
        assert!(
            Instant::now() < deadline,
            "rexia never reconnected to a new server"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let ran = std::fs::read_to_string(&log).unwrap();
    println!("rex {}", ran.trim());
    println!("server instance {before} -> {after}; rexia reconnected");
    assert_eq!(ran.trim(), format!("-S {address} server start"));
    assert_eq!(h.workspace(|ws| ws.notice.clone()), None);
    drop(sessions);
}
