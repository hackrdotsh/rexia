//! ssh hosts that refuse: a fake ssh (named by REXIA_SSH) turns down `badkey`'s host key and
//! `badlogin`'s login, counting each try. Rexia stops after three refusals in a row with a
//! notice that says what to check, tries no more on its own (past the longest backoff),
//! and tries again on Retry. Needs no Rex server.
use rexia::rex::{
    client::Identity,
    endpoint::Endpoint,
    host::{Connection, HostCommand, HostHandle},
};
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

fn fake_ssh(dir: &Path) -> std::path::PathBuf {
    let script = dir.join("ssh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
for a; do dest=$prev; prev=$a; done
case "$*" in *dial-stdio*) ;; *) exit 0 ;; esac
case "$dest" in
  *badkey) echo try >> '{dir}/badkey.log'
    echo 'Host key verification failed.' >&2 ;;
  *badlogin) echo try >> '{dir}/badlogin.log'
    echo "tester@badlogin: Permission denied (publickey,password)." >&2 ;;
esac
exit 255
"#,
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn tries(dir: &Path, host: &str) -> usize {
    std::fs::read_to_string(dir.join(format!("{host}.log")))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

fn reason(handle: &HostHandle) -> String {
    match handle.snapshot().connection {
        Connection::Disconnected(reason) => reason,
        other => format!("{other:?}"),
    }
}

fn wait_for(handle: &HostHandle, what: &str, check: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let now = reason(handle);
        if check(&now) {
            return now;
        }
        assert!(Instant::now() < deadline, "no {what}; last: {now}");
        let _ = handle.wake.recv_timeout(Duration::from_millis(100));
    }
}

#[test]
fn refused_ssh_hosts_stop_after_three_tries() {
    let dir = tempfile::tempdir().unwrap();
    let ssh = fake_ssh(dir.path());
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_SSH", &ssh) };
    let open = |host: &str| {
        HostHandle::spawn(
            Endpoint::parse(&format!("ssh://tester@{host}")).unwrap(),
            Identity::default(),
            None,
        )
    };
    let (key, login) = (open("badkey"), open("badlogin"));

    let key_notice = wait_for(&key, "stop for the host key", |r| r.starts_with("Stopped"));
    let login_notice = wait_for(&login, "stop for the login", |r| r.starts_with("Stopped"));
    println!(
        "badkey after {} tries: {key_notice}",
        tries(dir.path(), "badkey")
    );
    println!(
        "badlogin after {} tries: {login_notice}",
        tries(dir.path(), "badlogin")
    );
    assert!(key_notice.contains("ssh-keygen -R badkey"));
    assert!(login_notice.contains("badlogin refused the login"));
    assert_eq!(tries(dir.path(), "badkey"), 3);
    assert_eq!(tries(dir.path(), "badlogin"), 3);

    // Past the longest backoff (8 s), neither has tried again.
    std::thread::sleep(Duration::from_secs(10));
    println!(
        "10 s later: badkey {} tries, badlogin {}",
        tries(dir.path(), "badkey"),
        tries(dir.path(), "badlogin")
    );
    assert_eq!(tries(dir.path(), "badkey"), 3);
    assert_eq!(tries(dir.path(), "badlogin"), 3);

    // Retry tries again, and counts afresh: one refusal shows ssh's own words.
    key.send(HostCommand::Reconnect);
    let retried = wait_for(&key, "a fresh try", |r| {
        !r.starts_with("Stopped") && r.contains("Host key")
    });
    println!("Retry: try {}, {retried}", tries(dir.path(), "badkey"));
    assert_eq!(tries(dir.path(), "badkey"), 4);
}
