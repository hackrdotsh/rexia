//! Download a regular file from a terminal's host without executing a remote shell.
use super::{data, endpoint, host::HostHandle, upload};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::Message;

/// A safe directory component for a host label; labels are not local paths.
pub fn host_directory(label: &str) -> String {
    let name: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() { "host".into() } else { name }
}

/// The default destination for copies opened from remote terminals.
pub fn directory(label: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Downloads/Rex")
        .join(host_directory(label)))
}

/// Save a complete file in `directory`. Existing files are never overwritten, and a
/// failed stream leaves no partial download. The server resolves relative paths and ~.
pub async fn download(
    host: &HostHandle,
    session: &str,
    block: &str,
    path: &str,
    directory: &Path,
) -> Result<PathBuf> {
    let reply = host
        .request(
            "com.superlogical.terminal.download_begin",
            json!({"session_id": session, "block_id": block,
            "args": {"path": path, "ref": format!("download:{}", upload::uuid())}}),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let name = reply["name"]
        .as_str()
        .context("the host gave no file name")?;
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
        bail!("the host gave an invalid file name");
    }
    let ticket = reply["ticket"]
        .as_str()
        .context("the host gave no download ticket")?;
    let mut socket = endpoint::connect(&host.endpoint, "/api/data", upload::PROTOCOL).await?;
    data::handshake(&mut socket, ticket).await?;
    tokio::fs::create_dir_all(directory).await?;
    // An exclusive reservation preserves user files, including symlinks with this name.
    let mut destination = directory.join(name);
    let mut file = loop {
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&destination)
            .await
        {
            Ok(file) => break file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let name = Path::new(name);
                let stem = name.file_stem().unwrap_or_default().to_string_lossy();
                let extension = name
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                destination = directory.join(format!("{stem}-{}{extension}", upload::uuid()));
            }
            Err(error) => return Err(error.into()),
        }
    };
    let result: Result<()> = async {
        loop {
            let message = tokio::time::timeout(std::time::Duration::from_secs(30), socket.next())
                .await
                .context("the download stopped responding")?;
            match message {
                Some(Ok(Message::Binary(bytes))) => file.write_all(&bytes).await?,
                Some(Ok(Message::Close(Some(frame)))) if u16::from(frame.code) == 1000 => {
                    file.flush().await?;
                    return Ok(());
                }
                Some(Ok(Message::Close(frame))) => bail!("the download stopped: {frame:?}"),
                Some(Err(error)) => return Err(error.into()),
                None => bail!("the download ended without confirming success"),
                _ => {}
            }
        }
    }
    .await;
    drop(file);
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(error);
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_labels_do_not_escape_the_download_folder() {
        assert_eq!(host_directory("../../studio"), "______studio");
        assert_eq!(host_directory("studio"), "studio");
        assert_eq!(host_directory(""), "host");
    }
}
