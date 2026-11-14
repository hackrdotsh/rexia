//! The SSH agent ssh asks for keys when it reaches ssh:// hosts: ssh's own choice, an
//! agent found on this computer, or a socket the user names.
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::RwLock,
    time::Duration,
};

/// The agent socket chosen in Settings; `None` leaves it to ssh (`SSH_AUTH_SOCK` and
/// `IdentityAgent` in `~/.ssh/config`).
static CHOSEN: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Use `socket` for ssh:// hosts connected from now on, or ssh's own choice with `None`.
pub fn choose(socket: Option<PathBuf>) {
    *CHOSEN.write().expect("ssh agent choice") = socket;
}

/// The agent socket ssh is told to use, if one was chosen.
pub fn chosen() -> Option<PathBuf> {
    CHOSEN.read().expect("ssh agent choice").clone()
}

/// An agent found on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownAgent {
    pub name: &'static str,
    pub socket: PathBuf,
}

/// Agents with a socket on this computer now: the session's, then the usual places of
/// common agents. The same socket under two names is listed once.
pub fn known_agents() -> Vec<KnownAgent> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates: Vec<(&'static str, Option<PathBuf>)> = vec![(
        "This session's agent (SSH_AUTH_SOCK)",
        std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
    )];
    let under = |base: &Option<PathBuf>, rest: &str| base.as_ref().map(|b| b.join(rest));
    candidates.extend([
        (
            "OpenSSH agent (systemd)",
            under(&runtime, "ssh-agent.socket"),
        ),
        ("GnuPG agent", under(&runtime, "gnupg/S.gpg-agent.ssh")),
        ("GNOME Keyring", under(&runtime, "gcr/ssh")),
        ("GNOME Keyring (older)", under(&runtime, "keyring/ssh")),
        ("1Password", under(&home, ".1password/agent.sock")),
        ("Bitwarden", under(&home, ".bitwarden-ssh-agent.sock")),
    ]);
    let mut found: Vec<KnownAgent> = Vec::new();
    let mut seen = Vec::new();
    for (name, socket) in candidates {
        let Some(socket) = socket else { continue };
        if !is_socket(&socket) {
            continue;
        }
        let real = std::fs::canonicalize(&socket).unwrap_or_else(|_| socket.clone());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        found.push(KnownAgent { name, socket });
    }
    found
}

fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).is_ok_and(|m| m.file_type().is_socket())
}

/// Ask the agent at `socket` how many keys it holds, as `ssh-add -l` does.
pub fn key_count(socket: &Path) -> Result<usize, String> {
    const REQUEST_IDENTITIES: u8 = 11;
    const IDENTITIES_ANSWER: u8 = 12;
    const FAILURE: u8 = 5;
    let mut stream = UnixStream::connect(socket).map_err(|e| format!("doesn't answer: {e}"))?;
    let timeout = Some(Duration::from_secs(2));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    stream
        .write_all(&[0, 0, 0, 1, REQUEST_IDENTITIES])
        .map_err(|e| format!("doesn't answer: {e}"))?;
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|e| format!("doesn't answer: {e}"))?;
    let length = u32::from_be_bytes(length) as usize;
    if !(1..=256 * 1024).contains(&length) {
        return Err("answered with something that isn't an SSH agent's reply".into());
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("stopped answering: {e}"))?;
    match body[0] {
        IDENTITIES_ANSWER if body.len() >= 5 => {
            Ok(u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize)
        }
        FAILURE => Err("refused to list its keys".into()),
        _ => Err("answered with something that isn't an SSH agent's reply".into()),
    }
}

/// The agent's status in words: how many keys it holds, or why it can't say.
pub fn status(socket: &Path) -> String {
    match key_count(socket) {
        Ok(0) => "Answers, but holds no keys".into(),
        Ok(1) => "Answers, with 1 key".into(),
        Ok(n) => format!("Answers, with {n} keys"),
        Err(why) => format!("The agent {why}"),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A fake agent that answers a request for identities with `keys` keys, then closes.
    pub fn fake_agent(socket: &Path, keys: u32) -> std::thread::JoinHandle<()> {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0u8; 5];
                if stream.read_exact(&mut request).is_err() {
                    continue;
                }
                assert_eq!(request, [0, 0, 0, 1, 11]);
                let mut body = vec![12];
                body.extend(keys.to_be_bytes());
                let mut reply = (body.len() as u32).to_be_bytes().to_vec();
                reply.extend(body);
                let _ = stream.write_all(&reply);
            }
        })
    }

    #[test]
    fn agents_report_their_keys() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("agent.sock");
        let _agent = fake_agent(&socket, 2);
        assert_eq!(key_count(&socket), Ok(2));
        assert_eq!(status(&socket), "Answers, with 2 keys");
        let missing = dir.path().join("missing.sock");
        assert!(status(&missing).starts_with("The agent doesn't answer"));
    }

    #[test]
    fn known_agents_are_sockets_listed_once() {
        for agent in known_agents() {
            assert!(is_socket(&agent.socket), "{agent:?}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_socket(dir.path()));
    }
}
