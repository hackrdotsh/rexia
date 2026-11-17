//! ssh:// hosts beyond the stand-ins. First, a host whose rex closes without answering (as
//! one too old to relay does) gets an error that says so. Then, when REXIA_TEST_SSH_HOST
//! names a real machine (such as `infernape/tmp/rpui-test/rex.sock`, a disposable server
//! there), rexia goes there through the system ssh, opens a terminal, and runs `hostname`
//! in it: the far machine's name comes back (REXIA_TEST_SSH_HOSTNAME, or the host part).
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::{
    client::Identity,
    endpoint::Endpoint,
    host::{Connection, HostHandle},
};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

#[test]
fn ssh_hosts_say_why_and_reach_real_machines() {
    // A rex that ends at once, without a word, as an old one does.
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join("ssh");
    std::fs::write(&ssh, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_SSH", &ssh) };
    let quiet = HostHandle::spawn(
        Endpoint::parse("ssh://oldrex").unwrap(),
        Identity::default(),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let reason = loop {
        if let Connection::Disconnected(reason) = quiet.snapshot().connection {
            break reason;
        }
        assert!(Instant::now() < deadline, "no failure reported");
        let _ = quiet.wake.recv_timeout(Duration::from_millis(100));
    };
    println!("a rex that closes without answering: {reason}");
    assert!(reason.contains("the rex on oldrex closed the connection without answering"));
    drop(quiet);
    // SAFETY: as above.
    unsafe { std::env::remove_var("REXIA_SSH") };

    // A real machine, through the system ssh.
    let Ok(target) = std::env::var("REXIA_TEST_SSH_HOST") else {
        eprintln!("skipped the real host: set REXIA_TEST_SSH_HOST to HOST/path/to/rex.sock");
        return;
    };
    let address = format!("ssh://{target}");
    let expected = std::env::var("REXIA_TEST_SSH_HOSTNAME").unwrap_or_else(|_| {
        target
            .split('/')
            .next()
            .unwrap_or_default()
            .rsplit('@')
            .next()
            .unwrap_or_default()
            .to_owned()
    });
    let started = Instant::now();
    let control = common::connect(&address);
    println!("connected to {address} in {:?}", started.elapsed());
    let sessions = Sessions::create(&control, "rexia-real-ssh-host");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt on the far machine", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("hostname");
    h.press("enter");
    let name = h.wait("its hostname", |m| {
        focused_text(m)
            .lines()
            .map(str::trim)
            .find(|l| *l == expected)
            .map(str::to_owned)
    });
    println!("the terminal there says it's {name}");
    h.screenshot("ssh-real-host");
    drop(sessions);
}
