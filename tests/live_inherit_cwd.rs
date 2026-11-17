//! New terminals inherit their source pane's current directory, even without OSC 7.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::{data::TERMINAL_CREATOR, host::HostHandle};
use serde_json::json;

fn cwd(h: &mut Harness, control: &HostHandle, session: &str, block: &str) -> String {
    let reply = h
        .call_from(
            control,
            &format!("{TERMINAL_CREATOR}.process"),
            json!({"session_id": session, "block_id": block, "args": {}}),
        )
        .unwrap();
    reply["foreground"]["cwd"]
        .as_str()
        .expect("foreground cwd")
        .to_owned()
}

fn focus(h: &mut Harness, control: &HostHandle, session: &str, block: &str) {
    h.call_from(
        control,
        "session.focus_block",
        json!({"session_id": session, "block_id": block}),
    )
    .unwrap();
    h.wait("source focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(block)).then_some(())
    });
}

fn change_directory(h: &mut Harness, control: &HostHandle, session: &str, block: &str, path: &str) {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    h.type_text(&format!("cd {quoted}; printf '\\nCWD_READY\\n'"));
    h.press("enter");
    h.wait("directory command output", |m| {
        focused_text(m).contains("CWD_READY").then_some(())
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if cwd(h, control, session, block) == path {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "source never changed to {path}"
        );
    }
}

fn exercise(address: &str, initial: &str, first: &str, second: &str) {
    let local = rexia::rex::endpoint::Endpoint::parse(address)
        .unwrap()
        .is_local();
    let control = common::connect(address);
    let sessions = Sessions::create(&control, "rexia-inherit-cwd");
    let session = &sessions.ids[0];
    let created = common::request(
        &control,
        "session.new_window",
        json!({
            "session_id": session,
            "focus": true,
            "layout": {"block": {"flavor": rexia::rex::data::SHELL_FLAVOR, "options": {
                "shell": "none", "command": ["bash", "--noprofile", "--norc"], "cwd": initial
            }}}
        }),
    )
    .unwrap();
    let source = created["block_ids"][0].as_str().unwrap().to_owned();
    let mut config = common::test_settings(address, session);
    config.terminal.startup_command = "bash --noprofile --norc".into();
    let mut h = Harness::open_with(config, 1100., 700.);
    h.wait("source prompt", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(&source)
            && !focused_text(m).is_empty())
        .then_some(())
    });
    change_directory(&mut h, &control, session, &source, first);

    for (action, expected) in [
        ("pane.split.right", first),
        ("pane.split.down", second),
        ("pane.split.left", first),
        ("pane.split.up", second),
        ("client.tab.new", first),
        ("pane.new_floating", second),
    ] {
        focus(&mut h, &control, session, &source);
        change_directory(&mut h, &control, session, &source, expected);
        h.run(action, json!(null));
        let new = h.wait("new terminal", |m| {
            let block = m.view()?.active_window()?.focused_block_id.as_ref()?;
            (block != &source && !focused_text(m).is_empty()).then(|| block.clone())
        });
        assert_eq!(cwd(&mut h, &control, session, &new), expected, "{action}");
        if local {
            let process = h
                .call_from(
                    &control,
                    &format!("{TERMINAL_CREATOR}.process"),
                    json!({
                        "session_id": session, "block_id": new, "args": {}
                    }),
                )
                .unwrap();
            assert_eq!(
                rexia::rex::activity::Process::parse(&process)
                    .name
                    .as_deref(),
                Some("bash"),
                "startup command remains in effect"
            );
        }
    }

    // An explicitly targeted source wins over the currently focused pane.
    let focused = h
        .host
        .snapshot()
        .view()
        .unwrap()
        .active_window()
        .unwrap()
        .focused_block_id
        .clone()
        .unwrap();
    change_directory(&mut h, &control, session, &focused, first);
    let window = h.window;
    let source_for_action = source.clone();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.run_action(
                    "pane.split.right",
                    json!(null),
                    "test",
                    rexia::ui::workspace::Target {
                        session_id: Some(session.clone()),
                        block_id: Some(source_for_action),
                    },
                    window,
                    cx,
                )
                .unwrap();
            })
            .unwrap()
    });
    let new = h.wait("targeted split", |m| {
        let block = m.view()?.active_window()?.focused_block_id.as_ref()?;
        (block != &focused && block != &source && !focused_text(m).is_empty())
            .then(|| block.clone())
    });
    assert_eq!(
        cwd(&mut h, &control, session, &new),
        second,
        "explicit source directory"
    );
    h.screenshot(if local {
        "inherited-working-directory-local"
    } else {
        "inherited-working-directory-ssh"
    });
}

#[test]
fn new_terminals_follow_current_directory() {
    let Some(address) = common::server() else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("project 'with quotes' and spaces");
    let second = directory.path().join("another project");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    exercise(
        &address,
        directory.path().to_str().unwrap(),
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    );
}

#[test]
fn remote_terminals_follow_remote_directory() {
    let Ok(target) = std::env::var("REXIA_TEST_SSH_HOST") else {
        return;
    };
    exercise(&format!("ssh://{target}"), "/", "/tmp", "/var/tmp");
}
