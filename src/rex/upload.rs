//! Files for a terminal on another machine: dropped or pasted files, or a pasted image, go to
//! the terminal's machine through `com.superlogical.terminal.upload_begin`, and the terminal
//! types their paths once the last one arrives.
//!
//! Each file has its own data stream (`rex.data.bytes.v1`): HELLO with the upload's ticket,
//! the file's bytes as binary messages, then an empty binary message, which half-closes the
//! stream. The terminal ends every stream of a batch with status 1000 once it has typed the
//! paths; any other ending means nothing was typed and nothing was kept.
use super::{
    data,
    endpoint::{self, Endpoint},
    host::HostHandle,
};
use anyhow::{Context as _, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::io::AsyncReadExt;
use tokio_tungstenite::tungstenite::Message;

pub const PROTOCOL: &str = "rex.data.bytes.v1";

/// How much of a file goes in one message.
const CHUNK: usize = 64 * 1024;

/// A file to send: one on this machine, or bytes such as a pasted image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Path(PathBuf),
    Bytes { name: String, bytes: Arc<Vec<u8>> },
}

impl Source {
    pub fn name(&self) -> String {
        match self {
            Self::Path(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into()),
            Self::Bytes { name, .. } => name.clone(),
        }
    }

    fn size(&self) -> Result<u64> {
        Ok(match self {
            Self::Path(path) => std::fs::metadata(path)
                .with_context(|| format!("can't read {}", path.display()))?
                .len(),
            Self::Bytes { bytes, .. } => bytes.len() as u64,
        })
    }
}

/// How an upload is going, shared with whoever shows it.
#[derive(Default)]
pub struct Progress {
    pub sent: AtomicU64,
    pub total: AtomicU64,
    pub cancelled: AtomicBool,
}

impl Progress {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        Ok(())
    }
}

/// A new random (version 4) UUID, as upload refs and batches want.
pub fn uuid() -> String {
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        b = (seed ^ u128::from(std::process::id()) << 64).to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Send `files` to `block`'s machine as one batch, so their paths are typed together.
pub async fn upload(
    host: &HostHandle,
    session: &str,
    block: &str,
    files: Vec<Source>,
    progress: Arc<Progress>,
) -> Result<()> {
    let sizes = files.iter().map(Source::size).collect::<Result<Vec<_>>>()?;
    progress.total.store(sizes.iter().sum(), Ordering::SeqCst);
    let batch = uuid();
    let count = files.len();
    let mut streams = Vec::new();
    for (index, (file, size)) in files.into_iter().zip(sizes).enumerate() {
        progress.check()?;
        let mut args =
            json!({"ref": format!("upload:{}", uuid()), "name": file.name(), "size": size});
        if count > 1 {
            args["batch"] = json!(batch);
            args["count"] = json!(count);
            args["index"] = json!(index);
        }
        let reply = host
            .request(
                "com.superlogical.terminal.upload_begin",
                json!({"session_id": session, "block_id": block, "args": args}),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let ticket = reply["ticket"]
            .as_str()
            .context("the terminal gave no upload ticket")?
            .to_owned();
        let mut socket = open(&host.endpoint, &ticket).await?;
        send(&mut socket, &file, &progress).await?;
        // An empty message half-closes the stream: that's the whole file.
        socket.send(Message::Binary(Vec::new().into())).await?;
        streams.push(socket);
    }
    // The terminal ends each stream once it has typed every path in the batch.
    for mut socket in streams {
        let ended = loop {
            tokio::select! {
                message = socket.next() => match message {
                    Some(Ok(Message::Close(frame))) => break frame.map(|f| (u16::from(f.code), f.reason.to_string())),
                    Some(Ok(_)) => {}
                    Some(Err(error)) => bail!("the upload broke off: {error}"),
                    None => break None,
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => progress.check()?,
            }
        };
        match ended {
            Some((1000, _)) => {}
            Some((code, reason)) => bail!("the terminal didn't take the files ({code} {reason})"),
            None => bail!("the upload broke off"),
        }
    }
    Ok(())
}

async fn open(endpoint: &Endpoint, ticket: &str) -> Result<endpoint::Socket> {
    let mut socket = endpoint::connect(endpoint, "/api/data", PROTOCOL).await?;
    data::handshake(&mut socket, ticket).await?;
    Ok(socket)
}

async fn send(socket: &mut endpoint::Socket, file: &Source, progress: &Progress) -> Result<()> {
    match file {
        Source::Bytes { bytes, .. } => {
            for chunk in bytes.chunks(CHUNK) {
                progress.check()?;
                socket.send(Message::Binary(chunk.to_vec().into())).await?;
                progress
                    .sent
                    .fetch_add(chunk.len() as u64, Ordering::SeqCst);
            }
        }
        Source::Path(path) => {
            let mut reader = tokio::fs::File::open(path)
                .await
                .with_context(|| format!("can't read {}", path.display()))?;
            let mut buffer = vec![0u8; CHUNK];
            loop {
                progress.check()?;
                let n = reader.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                socket
                    .send(Message::Binary(buffer[..n].to_vec().into()))
                    .await?;
                progress.sent.fetch_add(n as u64, Ordering::SeqCst);
            }
        }
    }
    Ok(())
}

/// A path as a shell reads it: as is when it's plain, else in single quotes.
pub fn shell_quote(path: &str) -> String {
    let plain = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./~:@%+=,-".contains(c));
    if plain {
        path.to_owned()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// Paths a file manager copied, written as `file://` URIs one per line, or `None` when the
/// text is anything else.
pub fn copied_files(text: &str) -> Option<Vec<PathBuf>> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if lines.is_empty() {
        return None;
    }
    lines
        .iter()
        .map(|l| url::Url::parse(l).ok()?.to_file_path().ok())
        .collect()
}

/// Whether pasting `text` could run a command before it can be read: a line break when the
/// program hasn't asked for bracketed paste, or anything that would end a bracketed paste.
pub fn unsafe_paste(text: &str, bracketed: bool) -> bool {
    text.contains("\x1b[201~") || (!bracketed && (text.contains('\n') || text.contains('\r')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_copied_files_and_unsafe_pastes() {
        assert_eq!(shell_quote("/tmp/notes.md"), "/tmp/notes.md");
        assert_eq!(shell_quote("/tmp/a b.md"), "'/tmp/a b.md'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(
            copied_files("file:///tmp/a%20b.md\r\nfile:///tmp/c\n"),
            Some(vec![PathBuf::from("/tmp/a b.md"), PathBuf::from("/tmp/c")])
        );
        assert_eq!(copied_files("just words"), None);
        assert!(unsafe_paste("rm -rf x\n", false));
        assert!(!unsafe_paste("rm -rf x\n", true));
        assert!(unsafe_paste("a\x1b[201~b", true));
        assert!(!unsafe_paste("one line", false));
        let id = uuid();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }
}
