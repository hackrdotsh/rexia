//! The SSH agent picker: choosing an agent in Settings, its status, and ssh being told to
//! use it, through a stand-in ssh that runs the remote command on this machine. Needs
//! REXIA_TEST_SERVER (a unix:// test server) and REXIA_TEST_REX. Never reaches a real host.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    rex::{client::Identity, endpoint::Endpoint, host::HostHandle},
    ui::workspace::SettingsSection,
};
use serde_json::json;
use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// A stand-in agent that answers every request for identities with `keys` keys.
fn fake_agent(socket: &Path, keys: u32) {
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = [0u8; 5];
            if stream.read_exact(&mut request).is_err() || request != [0, 0, 0, 1, 11] {
                continue;
            }
            let mut reply = 5u32.to_be_bytes().to_vec();
            reply.push(12);
            reply.extend(keys.to_be_bytes());
            let _ = stream.write_all(&reply);
        }
    });
}

/// A stand-in ssh: it records its arguments and SSH_AUTH_SOCK, then runs the remote
/// command with the test rex on PATH.
fn fake_ssh(dir: &Path, rex: &str) -> (PathBuf, PathBuf) {
    let rex_dir = Path::new(rex).parent().unwrap().display().to_string();
    let script = dir.join("ssh");
    let log = dir.join("ssh.log");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
printf 'args %s\nagent %s\n' "$*" "$SSH_AUTH_SOCK" >> '{log}'
for a; do last=$a; done
PATH='{rex_dir}':"$PATH"; export PATH
exec sh -c "$last"
"#,
            log = log.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, log)
}

/// Connect to `address` and wait until it's connected; what ssh was given, then.
fn connect_over_ssh(address: &str, log: &Path) -> String {
    let _ = std::fs::remove_file(log);
    let host = HostHandle::spawn(Endpoint::parse(address).unwrap(), Identity::default(), None);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !matches!(
        host.snapshot().connection,
        rexia::rex::host::Connection::Connected
    ) {
        assert!(
            Instant::now() < deadline,
            "no connection: {:?}",
            host.snapshot().connection
        );
        let _ = host.wake.recv_timeout(Duration::from_millis(100));
    }
    drop(host);
    std::fs::read_to_string(log).unwrap()
}

#[test]
fn the_chosen_agent_reaches_ssh() {
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
    let agent = dir.path().join("agent.sock");
    fake_agent(&agent, 3);
    let (ssh, log) = fake_ssh(dir.path(), &rex);
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_SSH", &ssh) };

    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-ssh-agent");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.run("client.settings", json!({}));
    let window = h.window;
    let choose = |h: &mut Harness, socket: Option<PathBuf>| {
        h.cx.update(|cx| {
            window
                .update(cx, |ws, window, cx| {
                    ws.show_settings_section(SettingsSection::Hosts, window, cx);
                    ws.choose_ssh_agent(socket, cx);
                })
                .unwrap()
        });
        h.wait_ui("the agent's status", |ws| {
            ws.ssh_agent_status().map(str::to_owned)
        })
    };

    // The chosen agent answers with its keys, and ssh is told to use it.
    let status = choose(&mut h, Some(agent.clone()));
    println!("chosen agent: {status}");
    assert_eq!(status, "Answers, with 3 keys");
    h.screenshot("settings-ssh-agent");
    let target = format!("ssh://tester@localhost{socket}");
    let given = connect_over_ssh(&target, &log);
    println!("ssh was given:\n{given}");
    let expected = format!("-o IdentityAgent={}", agent.display());
    assert!(given.contains(&expected), "{given}");
    assert!(
        given.contains(&format!("agent {}", agent.display())),
        "{given}"
    );

    // ssh's default: no IdentityAgent, so ssh's own environment and config decide.
    let status = choose(&mut h, None);
    println!("ssh's default: {status}");
    let given = connect_over_ssh(&target, &log);
    assert!(!given.contains("IdentityAgent"), "{given}");

    // A socket nothing listens on says so.
    let status = choose(&mut h, Some(dir.path().join("missing.sock")));
    println!("missing socket: {status}");
    assert!(status.starts_with("The agent doesn't answer"));
    drop(sessions);
}
