//! An ssh:// host through real OpenSSH: a throwaway sshd on 127.0.0.1, run as this user
//! with its own host key, and a client key with a passphrase. ssh asks rexia to trust the
//! host key and for the passphrase, once; the terminal's stream shares that connection.
//! The forced command runs the test rex against REXIA_TEST_SERVER. Skips without sshd.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{rex::askpass::Kind, ui::workspace::Workspace};
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const PASSPHRASE: &str = "correct horse 7";

fn which(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(program))
        .find(|p| p.exists())
        .and_then(|p| p.canonicalize().ok())
}

fn write_exec(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Close the connection ssh shares between rexia's streams, rather than let it linger.
struct CloseShared(PathBuf, u16);

impl Drop for CloseShared {
    fn drop(&mut self) {
        let dir = rexia::rex::askpass::runtime_dir().unwrap();
        let _ = Command::new(&self.0)
            .arg("-o")
            .arg(format!("ControlPath={}/ssh-%C", dir.display()))
            .args(["-O", "exit", "-p", &self.1.to_string(), "127.0.0.1"])
            .stderr(Stdio::null())
            .status();
    }
}

struct Sshd(Child);

impl Drop for Sshd {
    fn drop(&mut self) {
        // sshd's group holds the connections' processes too; ending them ends ssh's
        // shared connection on this side.
        let _ = Command::new("kill")
            .arg("--")
            .arg(format!("-{}", self.0.id()))
            .status();
        let _ = self.0.wait();
    }
}

/// Start sshd in `dir` on a free port; it lets in `id.pub` and runs `forced` for it.
fn start_sshd(dir: &Path, sshd: &Path, socket: &str, rex: &str) -> (Sshd, u16) {
    let keygen = |file: &str, passphrase: &str| {
        let status = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", passphrase, "-f"])
            .arg(dir.join(file))
            .status()
            .unwrap();
        assert!(status.success());
    };
    keygen("host", "");
    keygen("id", PASSPHRASE);
    let forced = dir.join("forced");
    write_exec(
        &forced,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$SSH_ORIGINAL_COMMAND\" >> '{}'\nexec '{rex}' -S unix://{socket} __internal dial-stdio\n",
            dir.join("commands.log").display()
        ),
    );
    let key = std::fs::read_to_string(dir.join("id.pub")).unwrap();
    std::fs::write(
        dir.join("authorized_keys"),
        format!("command=\"{}\" {key}", forced.display()),
    )
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(
        dir.join("sshd_config"),
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host\nPidFile none\n\
             AuthorizedKeysFile {d}/authorized_keys\nStrictModes no\nUsePAM no\n\
             PasswordAuthentication no\nKbdInteractiveAuthentication no\n",
            d = dir.display()
        ),
    )
    .unwrap();
    let log = std::fs::File::create(dir.join("sshd.log")).unwrap();
    let child = Command::new(sshd)
        .process_group(0)
        .args(["-D", "-e", "-f"])
        .arg(dir.join("sshd_config"))
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "sshd didn't start");
        std::thread::sleep(Duration::from_millis(50));
    }
    (Sshd(child), port)
}

fn answer(h: &mut Harness, typed: Option<&str>) {
    let window = h.window;
    let typed = typed.map(str::to_owned);
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                if let Some(text) = &typed {
                    ws.type_ssh_answer(text, cx);
                }
                ws.answer_ssh(true, window, cx);
            })
            .unwrap()
    });
    h.pump();
}

#[test]
fn real_ssh_asks_once_and_shares_the_connection() {
    let Some(address) = common::server() else {
        return;
    };
    let (Some(socket), Ok(rex), Some(sshd), Some(ssh)) = (
        address.strip_prefix("unix://").map(str::to_owned),
        std::env::var("REXIA_TEST_REX"),
        which("sshd"),
        which("ssh"),
    ) else {
        eprintln!("skipped: needs a unix:// REXIA_TEST_SERVER, REXIA_TEST_REX, ssh and sshd");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_sshd, port) = start_sshd(dir.path(), &sshd, &socket, &rex);
    // Only this test's known_hosts and key: never the user's own, nor their agent.
    std::fs::write(
        dir.path().join("ssh_config"),
        format!(
            "Host *\n  UserKnownHostsFile {d}/known_hosts\n  GlobalKnownHostsFile /dev/null\n  \
             IdentityFile {d}/id\n  IdentitiesOnly yes\n  IdentityAgent none\n",
            d = dir.path().display()
        ),
    )
    .unwrap();
    let wrapper = dir.path().join("ssh");
    write_exec(
        &wrapper,
        &format!(
            "#!/bin/sh\nunset SSH_AUTH_SOCK\nexec '{}' -F '{}' \"$@\"\n",
            ssh.display(),
            dir.path().join("ssh_config").display()
        ),
    );
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe {
        std::env::set_var("REXIA_SSH", &wrapper);
        std::env::set_var("REXIA_ASKPASS_PROGRAM", env!("CARGO_BIN_EXE_rexia"));
    }
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-real-ssh");
    let ssh_address = format!("ssh://127.0.0.1:{port}");
    let _close = CloseShared(wrapper.clone(), port);
    let mut h = Harness::open(&ssh_address, &sessions.ids[0], 1000., 640.);

    let (host, prompt, kind) = h.wait_ui("the host key question", Workspace::ssh_question);
    println!("ssh asked about {host} ({kind:?}):\n{prompt}");
    assert_eq!(kind, Kind::Confirm);
    assert!(prompt.contains("authenticity of host '[127.0.0.1]"));
    answer(&mut h, None);
    let (_, prompt, kind) = h.wait_ui("the passphrase question", Workspace::ssh_question);
    println!("then ({kind:?}): {prompt}");
    assert_eq!(kind, Kind::Secret);
    assert!(prompt.starts_with("Enter passphrase for key"));
    answer(&mut h, Some(PASSPHRASE));

    h.wait("a prompt over real ssh", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("printf 'real-%s\\n' ssh");
    h.press("enter");
    h.wait("the command's output", |m| {
        focused_text(m).contains("real-ssh").then_some(())
    });
    assert!(h.workspace(|ws| ws.ssh_question().is_none()), "asked again");
    h.screenshot("ssh-real");

    let commands = std::fs::read_to_string(dir.path().join("commands.log")).unwrap();
    let sshd_log = std::fs::read_to_string(dir.path().join("sshd.log")).unwrap();
    let signed_in = sshd_log.matches("Accepted publickey").count();
    println!(
        "connections through ssh: {}; sign-ins at sshd: {signed_in}",
        commands.lines().count()
    );
    println!("first command: {}", commands.lines().next().unwrap_or(""));
    assert!(
        commands.lines().count() >= 2,
        "control and a terminal stream"
    );
    assert_eq!(signed_in, 1, "one shared connection");
    let known = std::fs::read_to_string(dir.path().join("known_hosts")).unwrap();
    assert!(known.contains("[127.0.0.1]"), "the trusted key was saved");
    drop(sessions);
}
