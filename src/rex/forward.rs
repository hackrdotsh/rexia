//! A loopback listener carried by the installed Rex CLI's forwarding protocol.
use super::{endpoint::Endpoint, install};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
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

async fn run(
    endpoint: Endpoint,
    session: String,
    url: Url,
    ready: oneshot::Sender<Result<Url, String>>,
    mut cancel: oneshot::Receiver<()>,
) {
    let mut command = Command::new(install::program());
    // Only the destination socket belongs in the child arguments, not the URL's query or path.
    let destination = format!(
        "{}:{}",
        url.host_str().unwrap(),
        url.port_or_known_default().unwrap()
    );
    command
        .args([
            "-S",
            &endpoint.to_string(),
            "--autostart=false",
            "forward",
            "open",
            "--session",
            &session,
            "--bind",
            "127.0.0.1",
            "--no-open",
            "--",
            &destination,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
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
    let address = match line {
        Ok(Ok(Some(line))) => Url::parse(&line)
            .ok()
            .filter(|url| loopback(url) && url.port().is_some())
            .and_then(|address| {
                let mut local = url;
                local.set_host(Some("127.0.0.1")).ok()?;
                local.set_port(address.port()).ok()?;
                Some(local)
            }),
        Ok(Ok(None)) => {
            let diagnostic = tokio::time::timeout(Duration::from_secs(1), errors)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let message = if diagnostic.is_empty() {
                "Rex exited before opening the forward".into()
            } else {
                diagnostic
            };
            let _ = ready.send(Err(message));
            stop(&mut child).await;
            return;
        }
        _ => None,
    };
    if let Some(address) = address {
        let _ = ready.send(Ok(address));
        tokio::select! { _ = child.wait() => return, _ = cancel => {} }
    } else {
        let _ = ready.send(Err(
            "Rex did not report a loopback address for the forward".into()
        ));
    }
    stop(&mut child).await;
}

async fn stop(child: &mut tokio::process::Child) {
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
