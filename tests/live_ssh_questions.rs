//! ssh's questions about an ssh:// host, answered in rexia, plus Retry and moving hosts in
//! the list. A stand-in for ssh asks through `SSH_ASKPASS`, as OpenSSH does without a
//! terminal: first whether to trust the host key, then a password. Needs REXIA_TEST_SERVER
//! (a unix:// test server) and REXIA_TEST_REX. Never reaches a real host.
mod common;

use common::{Harness, Sessions};
use rexia::{
    rex::{askpass::Kind, host::Connection},
    ui::workspace::{SettingsSection, Workspace},
};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

const LABEL: &str = "Questions";

/// A stand-in ssh: it asks about the host key until it's trusted, then for the password
/// `hunter2`, then runs the remote command with the test rex on PATH. `log` notes each step.
fn questioning_ssh(dir: &Path, rex: &str) -> std::path::PathBuf {
    let rex_dir = Path::new(rex).parent().unwrap().display().to_string();
    let script = dir.join("ssh");
    let template = r#"#!/bin/sh
for a; do last=$a; done
log='LOG'
if [ ! -e 'KNOWN' ]; then
  answer=$("$SSH_ASKPASS" "The authenticity of host 'qhost (192.0.2.7)' can't be established.
ED25519 key fingerprint is SHA256:2kGQ0d7xNw3y8l5Jm8pZt8h4c0rB6sYwq9a1EXAMPLE.
Are you sure you want to continue connecting (yes/no/[fingerprint])? ") || {
    echo declined-host-key >> "$log"; echo 'Host key verification failed.' >&2; exit 255; }
  [ "$answer" = yes ] || { echo "host-key-answer-$answer" >> "$log"; exit 255; }
  : > 'KNOWN'
  echo trusted >> "$log"
fi
password=$("$SSH_ASKPASS" "tester@qhost's password: ") || {
  echo declined-password >> "$log"; echo 'tester@qhost: Permission denied (password).' >&2; exit 255; }
if [ "$password" != hunter2 ]; then
  echo wrong-password >> "$log"; echo 'tester@qhost: Permission denied (password).' >&2; exit 255
fi
echo signed-in >> "$log"
PATH='REXDIR':"$PATH"; export PATH
exec sh -c "$last"
"#;
    let text = template
        .replace("LOG", &dir.join("steps.log").display().to_string())
        .replace("KNOWN", &dir.join("known").display().to_string())
        .replace("REXDIR", &rex_dir);
    std::fs::write(&script, text).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn steps(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("steps.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn index_of(ws: &Workspace) -> Option<usize> {
    ws.hosts.iter().position(|h| h.config.label == LABEL)
}

fn connection(ws: &Workspace) -> Option<Connection> {
    Some(ws.hosts[index_of(ws)?].handle.snapshot().connection)
}

/// Wait for ssh's next question, then return it.
fn question(h: &mut Harness, what: &str) -> (String, String, Kind) {
    h.wait_ui(what, Workspace::ssh_question)
}

fn answer(h: &mut Harness, typed: Option<&str>, yes: bool) {
    let window = h.window;
    let typed = typed.map(str::to_owned);
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                if let Some(text) = &typed {
                    ws.type_ssh_answer(text, cx);
                }
                ws.answer_ssh(yes, window, cx);
            })
            .unwrap()
    });
    h.pump();
}

struct ForgetHost(rexia::rex::host::HostHandle, String);

impl Drop for ForgetHost {
    fn drop(&mut self) {
        let _ = common::request(&self.0, "hosts.remove", json!({"id": self.1}));
    }
}

#[test]
fn ssh_questions_are_answered_declined_and_retried() {
    let Some(address) = common::server() else {
        return;
    };
    let (Some(socket), Ok(rex)) = (
        address.strip_prefix("unix://").map(str::to_owned),
        std::env::var("REXIA_TEST_REX"),
    ) else {
        eprintln!("skipped: needs a unix:// REXIA_TEST_SERVER and REXIA_TEST_REX");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ssh = questioning_ssh(dir.path(), &rex);
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe {
        std::env::set_var("REXIA_SSH", &ssh);
        std::env::set_var("REXIA_ASKPASS_PROGRAM", env!("CARGO_BIN_EXE_rexia"));
    }
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-ssh-questions");
    let mut h = Harness::open(&address, &sessions.ids[0], 1000., 700.);
    let put = common::request(
        &control,
        "hosts.put",
        json!({"host": {"label": LABEL,
            "endpoints": [format!("ssh://tester@qhost{socket}")]}}),
    )
    .unwrap();
    let id = put["host"]["id"].as_str().unwrap().to_owned();
    let _forget = ForgetHost(control.clone(), id.clone());

    // ssh asks whether to trust the host key, naming the host as rexia lists it.
    let (host, prompt, kind) = question(&mut h, "the host key question");
    println!("asked about {host}: {kind:?}\n{prompt}");
    assert_eq!(host, "Questions");
    assert_eq!(kind, Kind::Confirm);
    assert!(prompt.contains("authenticity of host 'qhost"));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.open_settings(window, cx);
                ws.show_settings_section(SettingsSection::Hosts, window, cx);
            })
            .unwrap()
    });
    h.pump();
    h.screenshot("ssh-question-host-key");

    // Declined: ssh fails, and reconnecting doesn't ask again until Retry.
    answer(&mut h, None, false);
    let failed = h.wait_ui("the failure", |ws| match connection(ws)? {
        Connection::Disconnected(reason) if reason.contains("Host key verification") => {
            Some(reason)
        }
        _ => None,
    });
    println!("declined: {failed}");
    let deadline = Instant::now() + Duration::from_secs(20);
    while steps(dir.path()).len() < 2 {
        h.pump();
        assert!(h.workspace(|ws| ws.ssh_question().is_none()), "asked again");
        assert!(Instant::now() < deadline, "no reconnect");
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("reconnects while declined: {:?}", steps(dir.path()));
    assert!(steps(dir.path()).iter().all(|s| s == "declined-host-key"));
    h.screenshot("ssh-question-declined");

    // Retry asks again; trusting the key leads to the password.
    let index = h.workspace(|ws| index_of(ws).unwrap());
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.retry_host(index, cx))
            .unwrap()
    });
    let (_, prompt, kind) = question(&mut h, "the host key question after Retry");
    assert_eq!(kind, Kind::Confirm);
    assert!(prompt.contains("authenticity"));
    answer(&mut h, None, true);
    let (_, prompt, kind) = question(&mut h, "the password question");
    println!("then: {kind:?} {prompt:?}");
    assert_eq!(
        (prompt.as_str(), kind),
        ("tester@qhost's password:", Kind::Secret)
    );

    // A wrong password fails, and ssh asks again when rexia reconnects.
    answer(&mut h, Some("letmein"), true);
    let (_, prompt, _) = question(&mut h, "the password, asked again");
    assert!(prompt.ends_with("password:"));
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.type_ssh_answer("hunter2", cx))
            .unwrap()
    });
    h.pump();
    h.screenshot("ssh-question-password");
    answer(&mut h, None, true);
    h.wait_ui("the host connected", |ws| {
        matches!(connection(ws)?, Connection::Connected).then_some(())
    });
    let log = steps(dir.path());
    println!("ssh's steps: {log:?}");
    assert_eq!(log.last().map(String::as_str), Some("signed-in"));
    assert!(log.contains(&"wrong-password".to_owned()));

    // Moving the listed host above this computer, and keeping it there.
    let before = h.workspace(|ws| {
        ws.hosts
            .iter()
            .map(|h| h.config.label.clone())
            .collect::<Vec<_>>()
    });
    let index = h.workspace(|ws| index_of(ws).unwrap());
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.move_host(index, -1, cx))
            .unwrap()
    });
    for _ in 0..5 {
        h.pump();
    }
    let (after, order, active) = h.workspace(|ws| {
        (
            ws.hosts
                .iter()
                .map(|h| h.config.label.clone())
                .collect::<Vec<_>>(),
            ws.settings.host_order.clone(),
            ws.host().config.label.clone(),
        )
    });
    println!("hosts {before:?} -> {after:?}; saved order {order:?}; showing {active}");
    assert_eq!(after.first().map(String::as_str), Some("Questions"));
    assert_eq!(order.first(), Some(&id));
    assert_eq!(
        active, "flareon",
        "moving a host doesn't change the one shown"
    );
    h.screenshot("ssh-hosts-moved");
    drop(sessions);
}
