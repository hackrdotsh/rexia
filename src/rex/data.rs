//! The terminal data channel (`rex.data.terminal.v1`): ticket handshake and resume, the
//! Ghostty snapshot that opens a stream, and Rex's in-band APC metadata.
use super::endpoint::Socket;
use anyhow::{Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

pub const PROTOCOL: &str = "rex.data.terminal.v1";
pub const TERMINAL_CREATOR: &str = "com.superlogical.terminal";
pub const SHELL_FLAVOR: &str = "com.superlogical.terminal.shell";

/// The server refused a data handshake, with its reason byte.
#[derive(Debug)]
pub struct Rejected(pub u8);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rex refused the terminal stream (reason {})", self.0)
    }
}

impl std::error::Error for Rejected {}

/// The data socket closed, with its WebSocket close code.
#[derive(Debug)]
pub struct Closed {
    pub code: Option<u16>,
    pub reason: String,
}

impl Closed {
    /// 4000: the block itself is gone, so reconnecting can't help.
    pub fn is_final(&self) -> bool {
        self.code == Some(4000)
    }
}

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "terminal stream closed")?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for Closed {}

#[derive(Clone)]
pub struct Welcome {
    pub token: [u8; 16],
    pub nonce: u64,
}

// The resume token is a credential, so it never appears in logs.
impl std::fmt::Debug for Welcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Welcome")
            .field("nonce", &self.nonce)
            .finish_non_exhaustive()
    }
}

pub async fn handshake(socket: &mut Socket, ticket: &str) -> Result<Welcome> {
    let ticket = URL_SAFE_NO_PAD.decode(ticket)?;
    ensure!(ticket.len() == 16, "invalid data ticket");
    let mut hello = vec![0x01];
    hello.extend(ticket);
    exchange(socket, hello).await
}

pub async fn resume(socket: &mut Socket, token: &[u8; 16], offset: u64) -> Result<Welcome> {
    let mut hello = vec![0x02];
    hello.extend(token);
    hello.extend(offset.to_le_bytes());
    exchange(socket, hello).await
}

async fn exchange(socket: &mut Socket, hello: Vec<u8>) -> Result<Welcome> {
    socket.send(Message::Binary(hello.into())).await?;
    let reply = tokio::time::timeout(std::time::Duration::from_secs(8), next_binary(socket))
        .await
        .map_err(|_| anyhow!("Rex did not answer the terminal handshake"))??;
    parse_welcome(&reply)
}

pub fn parse_welcome(reply: &[u8]) -> Result<Welcome> {
    if reply.first() == Some(&0x82) {
        return Err(Rejected(reply.get(1).copied().unwrap_or(0)).into());
    }
    ensure!(
        reply.len() == 32 && reply[0] == 0x81,
        "invalid terminal stream WELCOME"
    );
    let nonce = u64::from_le_bytes(reply[24..32].try_into()?);
    ensure!(nonce != 0, "invalid terminal stream nonce");
    Ok(Welcome {
        token: reply[1..17].try_into()?,
        nonce,
    })
}

pub async fn next_binary(socket: &mut Socket) -> Result<Vec<u8>> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => return Ok(bytes.to_vec()),
            Some(Ok(Message::Close(frame))) => {
                return Err(Closed {
                    code: frame.as_ref().map(|f| f.code.into()),
                    reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                }
                .into());
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error.into()),
            None => {
                return Err(Closed {
                    code: None,
                    reason: "the connection ended".into(),
                }
                .into());
            }
        }
    }
}

const SNAPSHOT_MAGIC: &[u8; 10] = b"GHOSTSNP\x01\x00";
const RECORD_HEADER: usize = 10;
const FINISH_TAG: u16 = 6;
const SNAPSHOT_LIMIT: usize = 256 * 1024 * 1024;

/// Collects a Ghostty snapshot that may arrive split across any number of frames, and finds
/// where it ends by its FINISH record, never by frame boundaries.
#[derive(Default)]
pub struct SnapshotBuffer {
    bytes: Vec<u8>,
    scan: usize,
}

impl SnapshotBuffer {
    /// Returns the complete snapshot and the stream bytes that followed it, once FINISH is in.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        ensure!(
            self.bytes.len() + bytes.len() <= SNAPSHOT_LIMIT,
            "terminal snapshot exceeds 256 MiB"
        );
        self.bytes.extend_from_slice(bytes);
        if self.scan == 0 {
            if self.bytes.len() < SNAPSHOT_MAGIC.len() {
                return Ok(None);
            }
            ensure!(
                self.bytes.starts_with(SNAPSHOT_MAGIC),
                "unsupported terminal snapshot format"
            );
            self.scan = SNAPSHOT_MAGIC.len();
        }
        while self.bytes.len() >= self.scan + RECORD_HEADER {
            let header = &self.bytes[self.scan..self.scan + RECORD_HEADER];
            let tag = u16::from_le_bytes([header[0], header[1]]);
            let len = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
            ensure!(
                len <= SNAPSHOT_LIMIT - self.scan - RECORD_HEADER,
                "terminal snapshot record exceeds its limit"
            );
            let end = self.scan + RECORD_HEADER + len;
            if end > self.bytes.len() {
                return Ok(None);
            }
            self.scan = end;
            if tag == FINISH_TAG {
                let mut snapshot = std::mem::take(&mut self.bytes);
                let tail = snapshot.split_off(end);
                self.scan = 0;
                return Ok(Some((snapshot, tail)));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Output(Vec<u8>),
    Metadata(Value),
}

const APC_LIMIT: usize = 1024 * 1024;

/// Separates Rex's metadata (`ESC _ rex1;<nonce>;<json> ESC \`) from terminal output. Only
/// an APC with this stream's nonce counts, so programs can't forge metadata. Partial
/// sequences are held across frames.
pub struct ApcParser {
    prefix: Vec<u8>,
    pending: Vec<u8>,
}

impl ApcParser {
    pub fn new(nonce: u64) -> Self {
        Self {
            prefix: format!("\x1b_rex1;{nonce};").into_bytes(),
            pending: Vec::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Part>> {
        let mut buffer = std::mem::take(&mut self.pending);
        buffer.extend_from_slice(bytes);
        let mut parts = Vec::new();
        let mut output = Vec::new();
        let mut at = 0;
        while at < buffer.len() {
            let Some(escape) = buffer[at..].iter().position(|b| *b == 0x1b).map(|i| at + i) else {
                output.extend_from_slice(&buffer[at..]);
                break;
            };
            output.extend_from_slice(&buffer[at..escape]);
            let rest = &buffer[escape..];
            if rest.len() < self.prefix.len() {
                if self.prefix.starts_with(rest) {
                    self.pending = rest.to_vec();
                    break;
                }
                output.push(0x1b);
                at = escape + 1;
                continue;
            }
            if !rest.starts_with(&self.prefix) {
                output.push(0x1b);
                at = escape + 1;
                continue;
            }
            let body = &rest[self.prefix.len()..];
            let Some(end) = body.windows(2).position(|w| w == b"\x1b\\") else {
                if rest.len() > APC_LIMIT {
                    bail!("Rex metadata exceeds 1 MiB");
                }
                self.pending = rest.to_vec();
                break;
            };
            if !output.is_empty() {
                parts.push(Part::Output(std::mem::take(&mut output)));
            }
            parts.push(Part::Metadata(serde_json::from_slice(&body[..end])?));
            at = escape + self.prefix.len() + end + 2;
        }
        if !output.is_empty() {
            parts.push(Part::Output(output));
        }
        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(parts: &[Part]) -> Vec<u8> {
        parts
            .iter()
            .filter_map(|p| match p {
                Part::Output(bytes) => Some(bytes.as_slice()),
                Part::Metadata(_) => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    #[test]
    fn metadata_survives_every_split() {
        let message = b"a\x1b[1mb\x1b_rex1;123;{\"type\":\"resize\",\"cols\":80}\x1b\\z";
        for split in 0..=message.len() {
            let mut parser = ApcParser::new(123);
            let mut parts = parser.feed(&message[..split]).unwrap();
            parts.extend(parser.feed(&message[split..]).unwrap());
            assert_eq!(output(&parts), b"a\x1b[1mbz", "split at {split}");
            let metadata: Vec<_> = parts
                .iter()
                .filter_map(|p| match p {
                    Part::Metadata(v) => Some(v["cols"].as_u64()),
                    _ => None,
                })
                .collect();
            assert_eq!(metadata, vec![Some(80)]);
        }
    }

    #[test]
    fn a_foreign_nonce_is_plain_output() {
        let message = b"\x1b_rex1;456;{}\x1b\\";
        let mut parser = ApcParser::new(123);
        assert_eq!(
            parser.feed(message).unwrap(),
            vec![Part::Output(message.to_vec())]
        );
    }

    #[test]
    fn welcome_and_reject() {
        let mut reply = vec![0x81];
        reply.extend([7; 16]);
        reply.extend([0; 3]);
        reply.extend(1000u32.to_le_bytes());
        reply.extend(42u64.to_le_bytes());
        let welcome = parse_welcome(&reply).unwrap();
        assert_eq!((welcome.token, welcome.nonce), ([7; 16], 42));
        let error = parse_welcome(&[0x82, 1]).unwrap_err();
        assert_eq!(error.downcast_ref::<Rejected>().unwrap().0, 1);
    }

    #[test]
    fn snapshot_ends_at_finish_in_any_chunking() {
        let terminal = ghostty_vt::Terminal::new(ghostty_vt::TerminalOptions {
            cols: 20,
            rows: 4,
            ..Default::default()
        })
        .unwrap();
        let snapshot = ghostty_vt::snapshot::encode(&terminal).unwrap().to_vec();
        let mut bytes = snapshot.clone();
        bytes.extend_from_slice(b"after");
        for size in [1, 3, 10, 64, bytes.len()] {
            let mut buffer = SnapshotBuffer::default();
            let mut done = None;
            for chunk in bytes.chunks(size) {
                if let Some(result) = buffer.push(chunk).unwrap() {
                    assert!(done.is_none());
                    done = Some(result);
                } else if let Some((_, tail)) = done.as_mut() {
                    tail.extend_from_slice(chunk);
                }
            }
            let (found, tail) = done.expect("snapshot completes");
            assert_eq!(found, snapshot);
            assert_eq!(tail, b"after");
        }
    }
}
