//! A loopback listener carried by the installed Rex CLI's forwarding protocol.
use super::{endpoint::Endpoint, install};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::oneshot,
};
use url::{Host, Url};

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

pub type Ready = oneshot::Receiver<Result<Url, String>>;

impl Forward {
    pub fn start(
        endpoint: Endpoint,
        session: String,
        url: Url,
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
                    Ok(runtime) => runtime.block_on(run(endpoint, session, url, ready, cancel)),
                    Err(error) => {
                        let _ = ready.send(Err(error.to_string()));
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
    ready: oneshot::Sender<Result<Url, String>>,
    mut cancel: oneshot::Receiver<()>,
) {
    let port = url.port_or_known_default().unwrap();
    // Keep the original path, query, and fragment out of the child arguments.
    let destination = format!("{}:{port}", url.host_str().unwrap());
    let mut local_port = port;
    loop {
        let mut child = match spawn_forward(&endpoint, &session, &destination, local_port) {
            Ok(child) => child,
            Err(error) => {
                let _ = ready.send(Err(format!("Couldn't start Rex: {error}")));
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
        let result = match line {
            Ok(Ok(Some(line))) => Url::parse(&line)
                .ok()
                .filter(loopback)
                .and_then(|address| address.port_or_known_default())
                .filter(|port| *port != 0)
                .and_then(|port| {
                    url.set_host(Some(LOCAL_HOST)).ok()?;
                    url.set_port(Some(port)).ok()?;
                    Some(url)
                })
                .ok_or_else(|| "Rex did not report a loopback address for the forward".into()),
            Ok(Ok(None)) => {
                let diagnostic = tokio::time::timeout(Duration::from_secs(1), errors)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();
                // Retry only a collision on the local listener, not a transport or target error.
                let bind_error =
                    format!("listen tcp {LOCAL_HOST}:{local_port}: bind: address already in use");
                if local_port != 0 && diagnostic.contains(&bind_error) {
                    stop(&mut child).await;
                    local_port = 0;
                    continue;
                }
                Err(if diagnostic.is_empty() {
                    "Rex exited before opening the forward".into()
                } else {
                    diagnostic
                })
            }
            _ => Err("Rex did not report a loopback address for the forward".into()),
        };
        let listening = result.is_ok();
        let _ = ready.send(result);
        if listening {
            tokio::select! { _ = child.wait() => return, _ = cancel => {} }
        }
        stop(&mut child).await;
        return;
    }
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
