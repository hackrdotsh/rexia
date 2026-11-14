//! Settings › Server › Remote Address and Tailscale, against a server this test starts
//! with REXIA_TEST_REX: a plain TCP listener, and built-in Tailscale pointed at a control
//! server that isn't there, so nothing leaves this computer. Screenshots go to
//! REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{SettingsSection, remote::remote_addresses};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child, std::path::PathBuf);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn update(
    h: &mut Harness,
    change: impl FnOnce(
        &mut rexia::ui::workspace::Workspace,
        &mut gpui::Window,
        &mut gpui::Context<rexia::ui::workspace::Workspace>,
    ),
) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| change(ws, window, cx))
            .unwrap()
    });
    h.pump();
}

fn tailscale(h: &mut Harness, what: &str, check: impl Fn(&Value) -> bool) -> Value {
    h.wait_ui(what, |ws| ws.tailscale_status().filter(|t| check(t)))
}

#[test]
fn remote_addresses_and_tailscale_on_and_off() {
    let Ok(rex) = std::env::var("REXIA_TEST_REX") else {
        eprintln!("skipped: set REXIA_TEST_REX to a rex binary");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rexia-tailscale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("rex.sock");
    let address = format!("unix://{}", socket.display());
    let config = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/init.lua");
    let child = Command::new(&rex)
        .args(["server", "run", "--listen", &address])
        .args(["--extra-listen", "127.0.0.1:0", "--allow-anonymous"])
        .arg("--state-dir")
        .arg(dir.join("state"))
        .args(["--config", config, "--restore=false"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child, dir.clone());
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-tailscale");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.run("client.settings", json!({}));
    update(&mut h, |ws, window, cx| {
        ws.show_settings_section(SettingsSection::Server, window, cx)
    });
    let report = h.wait_ui("the server's report", |ws| ws.server_report().cloned());

    // The TCP listener is a remote address; the Unix socket isn't.
    let remote = remote_addresses(report.status.as_ref().unwrap());
    println!("remote addresses: {remote:?}");
    assert_eq!(remote.len(), 1);
    assert!(remote[0].address.starts_with("http://127.0.0.1:"));
    assert_eq!(
        remote[0].admits,
        "Anyone who can reach it, with full access"
    );
    let off = tailscale(&mut h, "Tailscale's status", |t| t["state"] == "disabled");
    println!("tailscale before: {off}");
    h.screenshot("settings-remote");

    // Tags need tailnet members let in: refused before asking the server.
    update(&mut h, |ws, _, cx| {
        ws.set_tailscale_fields("rexia-test", "tag:rex", "http://127.0.0.1:9", cx);
        ws.set_tailscale(true, cx);
    });
    let refused = h.workspace(|ws| ws.tailscale_error().map(str::to_owned));
    println!("tags without members: {refused:?}");
    assert!(refused.is_some_and(|e| e.contains("tailnet members")));

    // On, with a control server that isn't there: it waits to sign in.
    update(&mut h, |ws, _, cx| {
        ws.set_tailscale_fields("rexia-test", "", "http://127.0.0.1:9", cx);
        ws.set_tailscale(true, cx);
    });
    let waiting = tailscale(&mut h, "Tailscale to need signing in", |t| {
        t["state"] == "needs_login"
    });
    println!("tailscale on: {waiting}");
    assert_eq!(waiting["control_url"], "http://127.0.0.1:9");
    let events = h.workspace(|ws| ws.model().tailscale_revision);
    println!("status events received: {events}");
    assert!(events > 0, "status changes arrive as events");
    // The section scrolls down to Tailscale's rows, under the pointer.
    h.input(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
        position: gpui::point(gpui::px(700.), gpui::px(400.)),
        pressed_button: None,
        modifiers: Default::default(),
    }));
    h.input(gpui::PlatformInput::Scroll(gpui::ScrollEvent {
        position: gpui::point(gpui::px(700.), gpui::px(400.)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-800.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    }));
    h.screenshot("settings-tailscale-on");

    // Off again.
    update(&mut h, |ws, _, cx| ws.set_tailscale(false, cx));
    let off = tailscale(&mut h, "Tailscale off", |t| t["state"] == "disabled");
    println!("tailscale off: {off}");
    assert_eq!(
        h.workspace(|ws| ws.tailscale_error().map(str::to_owned)),
        None
    );
    drop(sessions);
}
