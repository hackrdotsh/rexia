//! Where a Rex server listens, and how to open a WebSocket to it.
use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::client::IntoClientRequest};

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub type Socket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<Box<dyn Io>>>;

/// A server address: a Unix socket path, or an HTTP(S)/WS(S) URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Unix(String),
    Url(url::Url),
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
            "ssh" => bail!("ssh:// hosts aren't supported yet; expose the server over HTTPS"),
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
        Endpoint::Unix(_) => url::Url::parse("ws://localhost")?,
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
    let stream = io(endpoint).await?;
    let (socket, response) =
        tokio_tungstenite::client_async_tls_with_config(request, stream, None, None).await?;
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
        assert!(Endpoint::parse("ssh://me@box").is_err());
    }
}
