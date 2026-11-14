//! Settings › Server: status, version and compatibility, against a real Rex server named
//! by REXIA_TEST_SERVER. REXIA_TEST_REX names the rex installed. Screenshots go to
//! REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{SettingsSection, server_status::Finding};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn report(h: &mut Harness) -> rexia::ui::workspace::server_status::ServerReport {
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

#[test]
fn server_section_shows_status_version_and_compatibility() {
    let (Some(address), Ok(rex)) = (common::server(), std::env::var("REXIA_TEST_REX")) else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-server-status");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_REX", &rex) };
    h.run("client.settings", json!({}));

    // The installed rex is the one running: compatible, nothing to do.
    let current = report(&mut h);
    let line = h.workspace(|ws| ws.server_status_line());
    let version = current.version.as_ref().unwrap()["rex_version"].clone();
    let found = rexia::ui::workspace::server_status::findings(&current);
    println!("status: {line}");
    println!("version {version}, installed {:?}", current.installed);
    println!("findings: {found:?}");
    assert!(line.starts_with("Running on this computer for"), "{line}");
    assert_eq!(current.installed.as_deref(), version.as_str());
    assert!(matches!(found.as_slice(), [Finding::Compatible { .. }]));
    h.screenshot("settings-server");

    // After an update, the installed rex is newer than the server still running.
    let fake = std::env::temp_dir().join(format!("rexia-newer-rex-{}", std::process::id()));
    std::fs::write(&fake, "#!/bin/sh\necho 'rex 0.1.0-preview.99'\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: as above.
    unsafe { std::env::set_var("REXIA_REX", &fake) };
    let updated = report(&mut h);
    let found = rexia::ui::workspace::server_status::findings(&updated);
    println!("after an update: {found:?}");
    assert!(found.contains(&Finding::Outdated {
        running: version.as_str().unwrap().to_owned(),
        installed: "0.1.0-preview.99".into(),
    }));
    h.screenshot("settings-server-outdated");
    let _ = std::fs::remove_file(&fake);
    drop(sessions);
}
