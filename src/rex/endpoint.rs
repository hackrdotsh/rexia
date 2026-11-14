//! Where a Rex server listens, and how to open a WebSocket to it.
use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::client::IntoClientRequest};

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub type Socket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<Box<dyn Io>>>;

/// A server address: a Unix socket path, an HTTP(S)/WS(S) URL, or an ssh:// host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Unix(String),
    Url(url::Url),
    Ssh(SshTarget),
}

/// `ssh://[USER@]HOST[:PORT][/SOCKET]`: a server reached through the system's ssh, which
/// runs `rex __internal dial-stdio` on the host, as the Rex CLI does. Without a socket,
/// that finds (or starts) the ssh user's own server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub socket: Option<String>,
}

impl SshTarget {
    fn parse(url: &url::Url) -> Result<Self> {
        if url.as_str().contains('%') {
            bail!("an ssh:// address can't contain % escapes");
        }
        if url.query().is_some() || url.fragment().is_some() {
            bail!("an ssh:// address can't have a query or fragment");
        }
        if url.password().is_some() {
            bail!("an ssh:// address can't hold a password; ssh uses your keys");
        }
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .context("the address needs a host")?
            .to_owned();
        let socket = match url.path() {
            "" | "/" => None,
            path => {
                // The path goes into a shell command on the host.
                if !path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/._-+@:".contains(c))
                {
                    bail!("the socket path can only use letters, digits and / . _ - + @ :");
                }
                Some(path.to_owned())
            }
        };
        Ok(Self {
            user: (!url.username().is_empty()).then(|| url.username().to_owned()),
            host,
            port: url.port(),
            socket,
        })
    }

    /// The command ssh runs on the host. `sh -c` keeps it working whatever the login
    /// shell is, and `~/.local/bin` is where Rex's unprivileged install goes.
    pub fn remote_command(&self) -> String {
        let server = self
            .socket
            .as_ref()
            .map(|s| format!(" -S unix://{s}"))
            .unwrap_or_default();
        format!("sh -c 'PATH=\"$HOME/.local/bin:$PATH\"; exec rex{server} __internal dial-stdio'")
    }

    /// The arguments for ssh: no prompts (there's no terminal to answer them), keepalives
    /// so a dead link is noticed, then the host and the command.
    pub fn ssh_args(&self) -> Vec<String> {
        self.ssh_args_with(super::ssh_agent::chosen().as_deref())
    }

    /// ssh's arguments, telling it which agent to ask for keys when one was chosen.
    pub fn ssh_args_with(&self, agent: Option<&std::path::Path>) -> Vec<String> {
        let mut args: Vec<String> = [
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
        ]
        .map(String::from)
        .to_vec();
        if let Some(agent) = agent {
            let path = agent.display().to_string();
            // ssh splits option values at spaces unless they're quoted.
            let value = if path.contains(char::is_whitespace) {
                format!("\"{path}\"")
            } else {
                path
            };
            args.extend(["-o".into(), format!("IdentityAgent={value}")]);
        }
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string()]);
        }
        let destination = match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        };
        args.extend(["--".into(), destination, self.remote_command()]);
        args
    }
}

impl std::fmt::Display for SshTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ssh://")?;
        if let Some(user) = &self.user {
            write!(f, "{user}@")?;
        }
        write!(f, "{}", self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        write!(f, "{}", self.socket.as_deref().unwrap_or(""))
    }
}

/// ssh's standard input and output as one stream. The process ends with the stream.
struct SshStream {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl AsyncRead for SshStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for SshStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stdin).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stdin).poll_shutdown(cx)
    }
}

/// What ssh printed on standard error, for reporting why a connection failed.
type Stderr = std::sync::Arc<std::sync::Mutex<String>>;

/// Start ssh to `target`. `$REXIA_SSH` names another ssh program, for tests.
fn spawn_ssh(target: &SshTarget) -> Result<(SshStream, Stderr)> {
    use std::process::Stdio;
    let program = std::env::var("REXIA_SSH").unwrap_or_else(|_| "ssh".into());
    let agent = super::ssh_agent::chosen();
    let mut command = tokio::process::Command::new(&program);
    // Programs ssh starts locally, such as a ProxyCommand, find the same agent.
    if let Some(agent) = &agent {
        command.env("SSH_AUTH_SOCK", agent);
    }
    let mut child = command
        .args(target.ssh_args_with(agent.as_deref()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("couldn't run {program}"))?;
    let stdin = child.stdin.take().context("ssh has no standard input")?;
    let stdout = child.stdout.take().context("ssh has no standard output")?;
    let mut stderr = child.stderr.take().context("ssh has no standard error")?;
    let log: Stderr = Default::default();
    let sink = log.clone();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut chunk = [0u8; 1024];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            let mut text = sink.lock().expect("ssh stderr");
            if text.len() < 8192 {
                text.push_str(&String::from_utf8_lossy(&chunk[..n]));
            }
        }
    });
    Ok((
        SshStream {
            _child: child,
            stdin,
            stdout,
        },
        log,
    ))
}

/// The last thing ssh or the host's rex said, once it's had a moment to say it.
async fn ssh_failure(log: &Stderr) -> Option<String> {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let text = log.lock().expect("ssh stderr").clone();
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    Some(
        if line.contains("rex: not found") || line.contains("rex: command not found") {
            "rex isn't installed on the host, or isn't on its PATH or in ~/.local/bin".into()
        } else {
            line.to_owned()
        },
    )
}

impl Endpoint {
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        if let Some(path) = value.strip_prefix("unix://") {
            if !path.starts_with('/') {
                bail!("a unix:// endpoint needs an absolute path");
            }
            return Ok(Self::Unix(path.to_owned()));
        }
        if value.starts_with('/') {
            return Ok(Self::Unix(value.to_owned()));
        }
        let with_scheme = if value.contains("://") {
            value.to_owned()
        } else {
            format!("https://{value}")
        };
        let url = url::Url::parse(&with_scheme).context("invalid server address")?;
        match url.scheme() {
            "https" | "http" | "wss" | "ws" => {}
            "ssh" => return Ok(Self::Ssh(SshTarget::parse(&url)?)),
            other => bail!("unsupported address scheme {other}"),
        }
        if url.host_str().is_none() {
            bail!("the address needs a host");
        }
        Ok(Self::Url(url))
    }

    /// The local server's socket: `$REX_SERVER`, else `$XDG_RUNTIME_DIR/rex/server.sock`.
    pub fn local() -> Self {
        if let Ok(value) = std::env::var("REX_SERVER")
            && let Ok(endpoint) = Self::parse(&value)
        {
            return endpoint;
        }
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
            // SAFETY: getuid has no preconditions and cannot fail.
            format!("/run/user/{}", unsafe { libc_getuid() })
        });
        Self::Unix(format!("{runtime}/rex/server.sock"))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Unix(_))
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unix(path) => write!(f, "unix://{path}"),
            Self::Url(url) => write!(f, "{}", url.as_str().trim_end_matches('/')),
            Self::Ssh(target) => write!(f, "{target}"),
        }
    }
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

async fn io(endpoint: &Endpoint) -> Result<Box<dyn Io>> {
    match endpoint {
        Endpoint::Unix(path) => Ok(Box::new(
            tokio::net::UnixStream::connect(path)
                .await
                .with_context(|| format!("no Rex server at {path}"))?,
        )),
        Endpoint::Url(url) => {
            let host = url.host_str().context("address needs a host")?;
            let port = url
                .port_or_known_default()
                .context("address needs a port")?;
            let tcp = tokio::net::TcpStream::connect((host, port)).await?;
            tcp.set_nodelay(true)?;
            Ok(Box::new(tcp))
        }
        Endpoint::Ssh(_) => unreachable!("ssh hosts are opened by open()"),
    }
}

/// Open a WebSocket at `path` (such as `/api/control`) that speaks `protocol`.
pub async fn connect(endpoint: &Endpoint, path: &str, protocol: &str) -> Result<Socket> {
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        open(endpoint, path, protocol),
    )
    .await
    .context("connecting to Rex timed out")?
}

async fn open(endpoint: &Endpoint, path: &str, protocol: &str) -> Result<Socket> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut url = match endpoint {
        Endpoint::Unix(_) | Endpoint::Ssh(_) => url::Url::parse("ws://localhost")?,
        Endpoint::Url(url) => {
            let mut url = url.clone();
            let scheme = if matches!(url.scheme(), "https" | "wss") {
                "wss"
            } else {
                "ws"
            };
            url.set_scheme(scheme)
                .map_err(|_| anyhow::anyhow!("invalid address scheme"))?;
            url
        }
    };
    url.set_path(path);
    url.set_query(None);
    let mut request = url.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", protocol.parse()?);
    let (stream, ssh_log): (Box<dyn Io>, Option<Stderr>) = match endpoint {
        Endpoint::Ssh(target) => {
            let (stream, log) = spawn_ssh(target)?;
            (Box::new(stream), Some(log))
        }
        _ => (io(endpoint).await?, None),
    };
    let handshake =
        tokio_tungstenite::client_async_tls_with_config(request, stream, None, None).await;
    let (socket, response) = match (handshake, ssh_log) {
        (Ok(done), _) => done,
        (Err(error), Some(log)) => match ssh_failure(&log).await {
            Some(reason) => bail!("ssh: {reason}"),
            None => return Err(error.into()),
        },
        (Err(error), None) => return Err(error.into()),
    };
    let negotiated = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|value| value.to_str().ok());
    anyhow::ensure!(
        negotiated == Some(protocol),
        "the server did not accept {protocol}"
    );
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addresses() {
        assert_eq!(
            Endpoint::parse("unix:///run/rex.sock").unwrap(),
            Endpoint::Unix("/run/rex.sock".into())
        );
        assert_eq!(
            Endpoint::parse("/tmp/x.sock").unwrap(),
            Endpoint::Unix("/tmp/x.sock".into())
        );
        let host = Endpoint::parse("studio.tail.ts.net").unwrap();
        assert_eq!(host.to_string(), "https://studio.tail.ts.net");
        assert!(Endpoint::parse("unix://relative").is_err());
        assert!(Endpoint::parse("ftp://x").is_err());
    }

    #[test]
    fn parses_ssh_hosts_and_builds_the_command() {
        let Endpoint::Ssh(target) = Endpoint::parse("ssh://alice@build.example.com:2222").unwrap()
        else {
            panic!("not an ssh endpoint");
        };
        assert_eq!(target.user.as_deref(), Some("alice"));
        assert_eq!(target.port, Some(2222));
        assert_eq!(target.socket, None);
        assert_eq!(target.to_string(), "ssh://alice@build.example.com:2222");
        let args = target.ssh_args_with(None);
        assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
        assert!(!args.iter().any(|a| a.starts_with("IdentityAgent")));
        // A chosen agent goes to ssh as IdentityAgent, quoted when its path has spaces.
        let with = target.ssh_args_with(Some(std::path::Path::new("/run/user/1000/gcr/ssh")));
        assert!(
            with.windows(2)
                .any(|w| w == ["-o", "IdentityAgent=/run/user/1000/gcr/ssh"])
        );
        let spaced = target.ssh_args_with(Some(std::path::Path::new("/home/a b/agent.sock")));
        assert!(spaced.contains(&"IdentityAgent=\"/home/a b/agent.sock\"".to_owned()));
        assert_eq!(args[args.len() - 2], "alice@build.example.com");
        assert_eq!(
            args.last().unwrap(),
            "sh -c 'PATH=\"$HOME/.local/bin:$PATH\"; exec rex __internal dial-stdio'"
        );

        let Endpoint::Ssh(target) = Endpoint::parse("ssh://box/run/rex/server.sock").unwrap()
        else {
            panic!("not an ssh endpoint");
        };
        assert_eq!(target.socket.as_deref(), Some("/run/rex/server.sock"));
        assert!(
            target
                .remote_command()
                .contains("rex -S unix:///run/rex/server.sock __internal")
        );

        assert!(Endpoint::parse("ssh://box/tmp/a b").is_err());
        assert!(Endpoint::parse("ssh://box/tmp/%27").is_err());
        assert!(Endpoint::parse("ssh://me:secret@box").is_err());
        assert!(Endpoint::parse("ssh://").is_err());
    }
}
