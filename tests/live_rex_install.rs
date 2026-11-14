//! Settings › Developer › Rex Installation: the rex on PATH, or a build in a source
//! checkout, checked and then used for what rexia runs (here, the Server section's
//! installed version). Needs REXIA_TEST_SERVER and REXIA_TEST_REX.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::SettingsSection;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

fn checked(h: &mut Harness) -> Result<rexia::rex::install::Installation, String> {
    h.wait_ui("the installation check", |ws| ws.rex_check().cloned())
}

#[test]
fn rex_installation_is_chosen_checked_and_used() {
    let (Some(address), Ok(rex)) = (common::server(), std::env::var("REXIA_TEST_REX")) else {
        eprintln!("skipped: needs REXIA_TEST_SERVER and REXIA_TEST_REX");
        return;
    };
    // The test rex is the one on PATH; rexia picks rex itself, not $REXIA_REX.
    let rex_dir = PathBuf::from(&rex).parent().unwrap().to_owned();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![rex_dir];
    dirs.extend(std::env::split_paths(&path));
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe {
        std::env::set_var("PATH", std::env::join_paths(dirs).unwrap());
        std::env::remove_var("REXIA_REX");
    }
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-rex-install");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 900.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.run("client.settings", json!({}));
    let window = h.window;
    let show = |h: &mut Harness, section| {
        h.cx.update(|cx| {
            window
                .update(cx, |ws, window, cx| {
                    ws.show_settings_section(section, window, cx)
                })
                .unwrap()
        });
    };
    show(&mut h, SettingsSection::Developer);
    let on_path = checked(&mut h).expect("the rex on PATH runs");
    println!(
        "on PATH: rex {} at {} (sha256 {})",
        on_path.version,
        on_path.path.display(),
        &on_path.sha256[..12]
    );

    // A source checkout with a build in bin/: a wrapper that notes each run.
    let checkout = tempfile::tempdir().unwrap();
    let bin = checkout.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = checkout.path().join("runs.log");
    let wrapper = bin.join("rex");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{rex}' \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let dir = checkout.path().to_owned();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.choose_rex(Some(dir), cx))
            .unwrap()
    });
    let custom = checked(&mut h).expect("the checkout's rex runs");
    println!(
        "checkout: rex {} at {}",
        custom.version,
        custom.path.display()
    );
    assert_eq!(custom.path, wrapper);
    assert_ne!(custom.sha256, on_path.sha256);
    h.screenshot("settings-rex-installation");

    // What rexia runs now is the checkout's rex: the Server section asks it its version.
    std::fs::write(&log, "").unwrap();
    show(&mut h, SettingsSection::Server);
    let report = h.wait_ui("the server's report", |ws| ws.server_report().cloned());
    std::thread::sleep(Duration::from_millis(100));
    let runs = std::fs::read_to_string(&log).unwrap();
    println!(
        "the checkout's rex ran: {:?}; installed {:?}",
        runs.trim(),
        report.installed
    );
    assert!(runs.lines().any(|l| l == "version"));
    assert_eq!(report.installed.as_deref(), Some(on_path.version.as_str()));

    // A checkout without a build says so.
    let empty = tempfile::tempdir().unwrap();
    let dir = empty.path().to_owned();
    show(&mut h, SettingsSection::Developer);
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.choose_rex(Some(dir), cx))
            .unwrap()
    });
    let missing = checked(&mut h);
    println!("empty checkout: {missing:?}");
    assert!(missing.unwrap_err().starts_with("No rex executable"));
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.choose_rex(None, cx))
            .unwrap()
    });
    drop(sessions);
}
