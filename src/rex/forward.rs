//! A loopback listener carried by the installed Rex CLI's forwarding protocol.
use super::{endpoint::Endpoint, install};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::oneshot,
};
use url::{Host, Url};

mod privileged;

pub use privileged::available as authorization_available;

#[derive(Clone, Copy)]
pub enum LocalPort {
    Match,
    Free,
    Authorize,
}

pub enum StartError {
    PortChoice(Option<String>),
    Other(String),
}

pub fn target(input: &str) -> Result<Url, String> {
    let input = input.trim();
    let url = if input.starts_with("http://") || input.starts_with("https://") {
        Url::parse(input)
    } else if input.chars().all(|c| c.is_ascii_digit()) {
        Url::parse(&format!("http://localhost:{input}"))
    } else {
        Url::parse(&format!("http://{input}"))
    }
    .map_err(|_| "Enter a port, host:port, or HTTP(S) URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || url.port_or_known_default() == Some(0)
        || !url.username().is_empty()
        || url.password().is_some()
        || (!input.contains("://")
            && !input.chars().all(|c| c.is_ascii_digit())
            && !input.contains(':'))
    {
        return Err("Enter a port, host:port, or HTTP(S) URL without credentials".into());
    }
    Ok(url)
}

pub fn loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

pub struct Forward {
    // Dropping the sender closes the forward; the worker owns and reaps the child.
    _stop: oneshot::Sender<()>,
}

pub type Ready = oneshot::Receiver<Result<Url, StartError>>;

impl Forward {
    pub fn start(
        endpoint: Endpoint,
        session: String,
        url: Url,
        mode: LocalPort,
    ) -> std::io::Result<(Self, Ready, oneshot::Receiver<()>)> {
        let (stop, cancel) = oneshot::channel();
        let (ready, answer) = oneshot::channel();
        let (done, exited) = oneshot::channel();
        std::thread::Builder::new()
            .name("rex-forward".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        runtime.block_on(run(endpoint, session, url, mode, ready, cancel))
                    }
                    Err(error) => {
                        let _ = ready.send(Err(StartError::Other(error.to_string())));
                    }
                }
                let _ = done.send(());
            })?;
        Ok((Self { _stop: stop }, answer, exited))
    }
}

const LOCAL_HOST: &str = "127.0.0.1";

fn spawn_forward(
    endpoint: &Endpoint,
    session: &str,
    destination: &str,
    local_port: u16,
) -> std::io::Result<Child> {
    let mut command = Command::new(install::program());
    command
        .args([
            "-S",
            &endpoint.to_string(),
            "--autostart=false",
            "forward",
            "open",
            "--session",
            session,
            "--bind",
            LOCAL_HOST,
            "--port",
            &local_port.to_string(),
            "--no-open",
            "--",
            destination,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    {
        let parent = std::process::id();
        // SAFETY: the child only makes signal-safe syscalls before exec. Check for
        // the parent exiting before the kernel's parent-death signal is installed.
        unsafe {
            command.pre_exec(move || {
                unsafe extern "C" {
                    fn prctl(option: std::ffi::c_int, ...) -> std::ffi::c_int;
                    fn getppid() -> std::ffi::c_int;
                }
                const PR_SET_PDEATHSIG: std::ffi::c_int = 1;
                const SIGTERM: std::ffi::c_ulong = 15;
                if prctl(PR_SET_PDEATHSIG, SIGTERM, 0u64, 0u64, 0u64) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if getppid() as u32 != parent {
                    return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
                }
                Ok(())
            });
        }
    }
    command.spawn()
}

async fn run(
    endpoint: Endpoint,
    session: String,
    mut url: Url,
    mode: LocalPort,
    ready: oneshot::Sender<Result<Url, StartError>>,
    mut cancel: oneshot::Receiver<()>,
) {
    let port = url.port_or_known_default().unwrap();
    let listener = if matches!(mode, LocalPort::Authorize) {
        match tokio::select! {
            result = privileged::bind(port) => result,
            _ = &mut cancel => return,
        } {
            Ok(listener) => Some(listener),
            Err(error) => {
                let _ = ready.send(Err(StartError::PortChoice(Some(error))));
                return;
            }
        }
    } else {
        None
    };
    // Keep the original path, query, and fragment out of the child arguments.
    let destination = format!("{}:{port}", url.host_str().unwrap());
    let mut local_port = if matches!(mode, LocalPort::Match) {
        port
    } else {
        0
    };
    loop {
        let mut child = match spawn_forward(&endpoint, &session, &destination, local_port) {
            Ok(child) => child,
            Err(error) => {
                let _ = ready.send(Err(StartError::Other(format!(
                    "Couldn't start Rex: {error}"
                ))));
                return;
            }
        };
        let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut stderr = child.stderr.take().unwrap();
        let errors = tokio::spawn(async move {
            let mut text = Vec::new();
            let _ = (&mut stderr).take(8192).read_to_end(&mut text).await;
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            String::from_utf8_lossy(&text).trim().to_owned()
        });
        let line = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(20), stdout.next_line()) => result,
            _ = &mut cancel => { stop(&mut child).await; return; }
        };
        let upstream_port = match line {
            Ok(Ok(Some(line))) => Url::parse(&line)
                .ok()
                .filter(loopback)
                .and_then(|address| address.port_or_known_default())
                .filter(|port| *port != 0)
                .ok_or_else(invalid_address),
            Ok(Ok(None)) => {
                let diagnostic = tokio::time::timeout(Duration::from_secs(1), errors)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();
                // Retry only a collision on the local listener, not a transport or target error.
                let bind_error = format!("listen tcp {LOCAL_HOST}:{local_port}: bind: ");
                if local_port != 0
                    && diagnostic.contains(&format!("{bind_error}address already in use"))
                {
                    stop(&mut child).await;
                    local_port = 0;
                    continue;
                }
                if local_port != 0 && diagnostic.contains(&format!("{bind_error}permission denied"))
                {
                    stop(&mut child).await;
                    let _ = ready.send(Err(StartError::PortChoice(None)));
                    return;
                }
                Err(StartError::Other(if diagnostic.is_empty() {
                    "Rex exited before opening the forward".into()
                } else {
                    diagnostic
                }))
            }
            _ => Err(invalid_address()),
        };
        match upstream_port {
            Ok(upstream) => {
                url.set_host(Some(LOCAL_HOST)).expect("fixed IPv4 host");
                url.set_port(Some(if listener.is_some() { port } else { upstream }))
                    .expect("HTTP(S) URL");
                let _ = ready.send(Ok(url));
                if let Some(listener) = listener {
                    let upstream = (std::net::Ipv4Addr::LOCALHOST, upstream).into();
                    tokio::select! {
                        _ = child.wait() => return,
                        _ = privileged::relay(listener, upstream) => {},
                        _ = cancel => {},
                    }
                } else {
                    tokio::select! { _ = child.wait() => return, _ = cancel => {} }
                }
            }
            Err(error) => {
                let _ = ready.send(Err(error));
            }
        }
        stop(&mut child).await;
        return;
    }
}

fn invalid_address() -> StartError {
    StartError::Other("Rex did not report a loopback address for the forward".into())
}

async fn stop(child: &mut Child) {
    if let Some(pid) = child.id() {
        // Give Rex a chance to remove its forward block before forcing it to stop.
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .await;
    }
    if tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}
