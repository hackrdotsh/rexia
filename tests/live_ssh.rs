//! ssh:// hosts, through a stand-in for ssh that runs the remote command on this machine.
//! Needs REXIA_TEST_SERVER (a unix:// test server) and REXIA_TEST_REX (a rex binary that
//! speaks to it). Never reaches a real host.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::{client::Identity, endpoint::Endpoint, host::HostHandle};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

/// A fake ssh: it records its arguments, then runs the last one (the remote command) with
/// the test rex on PATH, except for the host `norex`, which has no rex installed.
fn fake_ssh(dir: &std::path::Path, rex: &str) -> std::path::PathBuf {
    let rex_dir = std::path::Path::new(rex)
        .parent()
        .unwrap()
        .display()
        .to_string();
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // The host without rex gets a PATH holding only sh, so a system-wide rex can't help it.
    let bare = dir.join("bare-bin");
    std::fs::create_dir_all(&bare).unwrap();
    let sh = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("sh"))
        .find(|p| p.exists())
        .expect("sh on PATH");
    std::os::unix::fs::symlink(sh, bare.join("sh")).unwrap();
    let script = dir.join("ssh");
    let log = dir.join("args.log");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
for a; do dest=$last; last=$a; done
export HOME='{home}'
case "$dest" in
  *norex) PATH='{bare}'; export PATH ;;
  *) PATH='{rex_dir}':"$PATH"; export PATH ;;
esac
exec sh -c "$last"
"#,
            log = log.display(),
            home = home.display(),
            bare = bare.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn ssh_hosts_stream_terminals_and_explain_failures() {
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
    let ssh = fake_ssh(dir.path(), &rex);
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_SSH", &ssh) };

    // A host without rex: the failure says so instead of a bare WebSocket error.
    let broken = HostHandle::spawn(
        Endpoint::parse("ssh://tester@norex").unwrap(),
        Identity::default(),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let reason = loop {
        let connection = format!("{:?}", broken.snapshot().connection);
        if connection.contains("rex isn't installed") {
            break connection;
        }
        assert!(
            Instant::now() < deadline,
            "no useful failure; last state: {connection}"
        );
        let _ = broken.wake.recv_timeout(Duration::from_millis(100));
    };
    println!("host without rex: {reason}");
    drop(broken);

    // A working host: rexia attaches over ssh and streams the terminal.
    let ssh_address = format!("ssh://tester@localhost{socket}");
    let control = common::connect(&ssh_address);
    let sessions = Sessions::create(&control, "rexia-ssh");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&ssh_address, &session, 1100., 700.);
    h.wait("a prompt over ssh", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("printf 'over-%s\\n' ssh");
    h.press("enter");
    h.wait("the command's output", |m| {
        focused_text(m).contains("over-ssh").then_some(())
    });
    h.screenshot("ssh-host");

    let args = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
    let first = args.lines().find(|l| l.contains("localhost")).unwrap();
    println!("ssh {first}");
    assert!(first.contains("-T -o BatchMode=yes"));
    assert!(first.contains(&format!("-- tester@localhost sh -c 'PATH=\"$HOME/.local/bin:$PATH\"; exec rex -S unix://{socket} __internal dial-stdio'")));
    println!(
        "ssh connections opened: {}",
        args.lines().filter(|l| l.contains("localhost")).count()
    );
    drop(sessions);
}
