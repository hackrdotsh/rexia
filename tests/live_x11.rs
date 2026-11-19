//! Opt-in native X11 regressions. See tests/fixtures/native_x11.py for dependencies.
use std::process::Command;

fn native(case: &str) {
    if std::env::var_os("REXIA_TEST_NATIVE_X11").is_none() {
        eprintln!("skipped: set REXIA_TEST_NATIVE_X11=1 to run with Xvfb and XTest");
        return;
    }
    let status = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/native_x11.py"
        ))
        .arg(env!("CARGO_BIN_EXE_rexia"))
        .arg(case)
        .status()
        .expect("launch the native X11 regression");
    assert!(status.success(), "native X11 {case} regression failed");
}

#[test]
fn activation_click_focuses_an_already_focused_terminal_without_forwarding() {
    native("focus");
}

#[test]
fn cold_tab_draws_all_its_terminals_with_its_first_layout() {
    native("tabs");
}

#[test]
fn stalled_tab_shows_its_connecting_panes_and_still_recovers() {
    native("stalled");
}
