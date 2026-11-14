//! Settings › Server's pending settings changes, against a server this test starts with
//! REXIA_TEST_REX through a stand-in `rex` that passes rexia's start options on: web client
//! origins pending until rexia restarts the server, and key bindings following config
//! reloads.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{
    SettingsSection,
    server_status::{Finding, ServerReport},
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

fn report(h: &mut Harness) -> ServerReport {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.show_settings_section(SettingsSection::Server, window, cx)
            })
            .unwrap()
    });
    h.wait_ui("the server's report", |ws| ws.server_report().cloned())
}

fn pending(h: &mut Harness) -> Vec<Finding> {
    let report = report(h);
    h.workspace(|ws| ws.pending_findings(&report))
}

#[test]
fn pending_origins_and_key_bindings() {
    let Ok(rex) = std::env::var("REXIA_TEST_REX") else {
        eprintln!("skipped: set REXIA_TEST_REX");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("rex.sock");
    let address = format!("unix://{}", socket.display());
    let config = dir.path().join("init.lua");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/init.lua"),
        &config,
    )
    .unwrap();
    let log = dir.path().join("rex.log");
    let fake = dir.path().join("rex");
    std::fs::write(
        &fake,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
[ "$3 $4" = "server start" ] || exec '{rex}' "$@"
endpoint=$2
shift 4
setsid '{rex}' server run --listen "$endpoint" --state-dir '{state}' --config '{config}' --restore=false "$@" > '{state}.log' 2>&1 < /dev/null &
i=0
until '{rex}' -S "$endpoint" --autostart=false server status > /dev/null 2>&1 || [ $i -ge 100 ]; do
  sleep 0.1; i=$((i+1))
done
"#,
            log = log.display(),
            state = dir.path().join("state").display(),
            config = config.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_REX", &fake) };
    // The server starts without rexia's help, as `rex server start` would.
    let started = std::process::Command::new(&fake)
        .args(["-S", &address, "server", "start"])
        .status()
        .unwrap();
    assert!(started.success());

    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-pending");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 900.);
    h.wait("a prompt and key bindings", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
    });
    h.run("client.settings", json!({}));
    let fresh = pending(&mut h);
    println!("at first: {fresh:?}");
    assert!(fresh.is_empty());

    // An origin added in Settings is pending: this server wasn't started by rexia.
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.add_origin("https://*.example.com", cx))
            .unwrap()
            .unwrap()
    });
    let added = pending(&mut h);
    println!("origin added: {added:?}");
    assert_eq!(
        added,
        [Finding::PendingOrigins {
            started_by_rexia: false
        }]
    );
    h.screenshot("pending-origins");

    // Restart Server applies it.
    let before = h.host.snapshot().server["instance_id"].clone();
    h.press("escape");
    h.run("client.server.restart", Value::Null);
    h.press("tab");
    h.press("enter");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.pump();
        let model = h.host.snapshot();
        if model.connection == rexia::rex::host::Connection::Connected
            && model.server["instance_id"] != before
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "rexia never reconnected to a new server"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    h.run("client.settings", json!({}));
    let deadline = Instant::now() + Duration::from_secs(20);
    let restarted = loop {
        let found = pending(&mut h);
        if !found
            .iter()
            .any(|f| matches!(f, Finding::PendingOrigins { .. }))
        {
            break found;
        }
        assert!(
            Instant::now() < deadline,
            "origins still pending: {found:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    let ran = std::fs::read_to_string(&log).unwrap();
    let start = ran
        .lines()
        .rfind(|l| l.contains("server start"))
        .unwrap()
        .to_owned();
    println!("rex ran: {start}");
    println!("after Restart Server: {restarted:?}");
    assert!(start.ends_with("--websocket-origin-pattern https://*.example.com"));

    // Removing it is pending again, until the next restart.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.remove_origin("https://*.example.com", cx)
            })
            .unwrap()
    });
    let removed = pending(&mut h);
    println!("origin removed: {removed:?}");
    assert_eq!(
        removed,
        [Finding::PendingOrigins {
            started_by_rexia: true
        }]
    );

    // Key bindings: a reload of a changed config file reaches rexia, so they stay current.
    let control = common::connect(&address);
    let old_hash = h.host.snapshot().keymap.unwrap()["config"]["hash"].clone();
    std::fs::write(
        &config,
        std::fs::read_to_string(&config).unwrap()
            + "\nrex.bind(\"ctrl+a>x\", \"client.tab.new\")\n",
    )
    .unwrap();
    common::request(&control, "server.config_reload", json!({})).unwrap();
    let new_hash = h.wait("the new key bindings", |m| {
        let hash = m.keymap.as_ref()?["config"]["hash"].clone();
        (hash != old_hash).then_some(hash)
    });
    let after = pending(&mut h);
    println!("config reloaded ({old_hash} -> {new_hash}): {after:?}");
    assert!(!after.contains(&Finding::StaleKeymap));
    let _ = common::request(&control, "server.stop", json!({}));
    drop(sessions);
}
