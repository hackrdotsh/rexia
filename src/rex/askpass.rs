//! Questions ssh asks while connecting to an ssh:// host: whether to trust a host key it
//! hasn't seen, a password, a key's passphrase. ssh has no terminal to ask on, so rexia is
//! its askpass program (`SSH_ASKPASS`): ssh runs rexia with the question, that rexia passes
//! it over a private socket to the running rexia, and the answer goes back the same way.
//!
//! A question someone declines stays declined for that host until they retry it, so the
//! reconnect loop doesn't ask again every few seconds.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// The environment variable that tells a rexia it was started by ssh to ask a question.
const SOCKET_VAR: &str = "REXIA_ASKPASS_SOCKET";
const HOST_VAR: &str = "REXIA_ASKPASS_HOST";

/// How long a question waits for an answer before ssh is told no.
const PATIENCE: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Text to type, hidden: a password or passphrase.
    Secret,
    /// Yes or no, such as whether to trust a new host key.
    Confirm,
    /// Something to know, with nothing to answer, such as "touch your security key".
    Notice,
}

#[derive(Serialize, Deserialize)]
struct Asked {
    host: String,
    prompt: String,
    kind: Kind,
}

#[derive(Serialize, Deserialize)]
struct Answered {
    answer: Option<String>,
}

/// A question from ssh, waiting for an answer.
pub struct Question {
    /// The host's address, as rexia gave it to ssh.
    pub host: String,
    pub prompt: String,
    pub kind: Kind,
    reply: flume::Sender<Option<String>>,
}

impl Question {
    /// Answer it: the text (`yes` or `no` for a confirmation), or `None` to decline. A
    /// declined host isn't asked again until [`retry`].
    pub fn answer(self, answer: Option<String>) {
        if answer.is_none() && self.kind != Kind::Notice {
            broker()
                .declined
                .lock()
                .expect("declined")
                .insert(self.host.clone());
        }
        let _ = self.reply.send(answer);
    }
}

impl std::fmt::Debug for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Question")
            .field("host", &self.host)
            .field("prompt", &self.prompt)
            .field("kind", &self.kind)
            .finish()
    }
}

struct Broker {
    socket: Option<PathBuf>,
    questions: (flume::Sender<Question>, flume::Receiver<Question>),
    declined: Mutex<HashSet<String>>,
}

fn broker() -> &'static Broker {
    static BROKER: OnceLock<Broker> = OnceLock::new();
    BROKER.get_or_init(|| {
        let questions = flume::unbounded();
        let socket = listen(questions.0.clone())
            .map_err(|error| eprintln!("rexia: can't answer ssh's questions: {error:#}"))
            .ok();
        Broker {
            socket,
            questions,
            declined: Mutex::default(),
        }
    })
}

/// The private directory rexia keeps its sockets in: `$XDG_RUNTIME_DIR/rexia`, or one
/// under the temporary directory. Only its owner can enter it.
pub fn runtime_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    // SAFETY: getuid has no preconditions and cannot fail.
    let dir = base.join(format!("rexia-{}", unsafe { getuid() }));
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

unsafe extern "C" {
    fn getuid() -> u32;
}

fn listen(questions: flume::Sender<Question>) -> anyhow::Result<PathBuf> {
    let dir = runtime_dir()?;
    // Sockets left by rexias that have quit.
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(pid) = name
            .strip_prefix("askpass-")
            .and_then(|n| n.strip_suffix(".sock"))
            && !Path::new(&format!("/proc/{pid}")).exists()
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let path = dir.join(format!("askpass-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::Builder::new()
        .name("rexia-askpass".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let questions = questions.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &questions);
                });
            }
        })?;
    Ok(path)
}

/// One question from one askpass rexia.
fn serve(stream: UnixStream, questions: &flume::Sender<Question>) -> anyhow::Result<()> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let asked: Asked = serde_json::from_str(&line)?;
    let declined = broker()
        .declined
        .lock()
        .expect("declined")
        .contains(&asked.host);
    let answer = if declined && asked.kind != Kind::Notice {
        None
    } else if asked.kind == Kind::Notice {
        // Nothing to answer: ssh carries on and closes the notice itself.
        let (reply, _) = flume::bounded(1);
        questions.send(Question {
            host: asked.host,
            prompt: asked.prompt,
            kind: asked.kind,
            reply,
        })?;
        None
    } else {
        let (reply, answer) = flume::bounded(1);
        questions.send(Question {
            host: asked.host,
            prompt: asked.prompt,
            kind: asked.kind,
            reply,
        })?;
        answer.recv_timeout(PATIENCE).ok().flatten()
    };
    let mut stream = stream;
    serde_json::to_writer(&mut stream, &Answered { answer })?;
    stream.write_all(b"\n")?;
    Ok(())
}

/// The questions ssh asks, as they come. Whoever shows them answers each one; a question
/// dropped unanswered is declined.
pub fn questions() -> flume::Receiver<Question> {
    broker().questions.1.clone()
}

/// Let `host` ask again, after someone declined one of its questions.
pub fn retry(host: &str) {
    broker().declined.lock().expect("declined").remove(host);
}

/// Whether someone declined one of `host`'s questions and hasn't retried it.
pub fn is_declined(host: &str) -> bool {
    broker().declined.lock().expect("declined").contains(host)
}

/// The environment for an ssh that asks rexia its questions about `host`, or nothing when
/// rexia can't take them (ssh then fails rather than waiting on a terminal it hasn't got).
pub fn ssh_env(host: &str) -> Vec<(String, String)> {
    let Some(socket) = broker().socket.as_ref() else {
        return Vec::new();
    };
    let program = std::env::var_os("REXIA_ASKPASS_PROGRAM")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok());
    let Some(program) = program else {
        return Vec::new();
    };
    vec![
        ("SSH_ASKPASS".into(), program.display().to_string()),
        ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        (SOCKET_VAR.into(), socket.display().to_string()),
        (HOST_VAR.into(), host.to_owned()),
    ]
}

/// When ssh started this rexia to ask a question: ask the running rexia, print its answer
/// for ssh, and return the exit status. `None` when this is an ordinary start.
pub fn helper() -> Option<i32> {
    let socket = std::env::var_os(SOCKET_VAR)?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let host = std::env::var(HOST_VAR).unwrap_or_default();
    // OpenSSH says what kind of answer it wants: `confirm` (the exit status says yes or
    // no), `none` (just show it), or nothing (print the answer).
    let hint = std::env::var("SSH_ASKPASS_PROMPT").unwrap_or_default();
    let kind = match hint.as_str() {
        "confirm" => Kind::Confirm,
        "none" => Kind::Notice,
        _ if prompt.contains("(yes/no") => Kind::Confirm,
        _ => Kind::Secret,
    };
    let ask = || -> anyhow::Result<Option<String>> {
        let mut stream = UnixStream::connect(&socket)?;
        serde_json::to_writer(
            &mut stream,
            &Asked {
                host,
                prompt: prompt.clone(),
                kind,
            },
        )?;
        stream.write_all(b"\n")?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        Ok(serde_json::from_str::<Answered>(&line)?.answer)
    };
    let answer = match ask() {
        Ok(answer) => answer,
        Err(error) => {
            eprintln!("rexia: couldn't ask about ssh's question: {error:#}");
            None
        }
    };
    Some(match (kind, answer) {
        (Kind::Notice, _) => 0,
        (_, None) => 1,
        (Kind::Confirm, Some(answer)) if hint == "confirm" => {
            i32::from(!answer.eq_ignore_ascii_case("yes"))
        }
        (_, Some(answer)) => {
            println!("{answer}");
            0
        }
    })
}
