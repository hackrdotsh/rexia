//! The control connection works, but the terminal data endpoint initially refuses connections.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::host::PaneStatus;
use std::{
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn relay(mut client: UnixStream, server: &Path, data_ready: &AtomicBool) -> std::io::Result<()> {
    let mut header = Vec::new();
    for _ in 0..8192 {
        let mut byte = [0];
        client.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if header.starts_with(b"GET /api/data ") && !data_ready.load(Ordering::Acquire) {
        return client.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
    }
    let mut upstream = UnixStream::connect(server)?;
    upstream.write_all(&header)?;
    let mut reader = client.try_clone()?;
    let mut writer = upstream.try_clone()?;
    let send = std::thread::spawn(move || {
        let _ = std::io::copy(&mut reader, &mut writer);
        let _ = writer.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream, &mut client);
    let _ = client.shutdown(Shutdown::Both);
    let _ = send.join();
    Ok(())
}

#[test]
fn terminal_recovers_when_a_temporarily_unavailable_data_endpoint_opens() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(socket) = address.strip_prefix("unix://") else {
        return;
    };
    let server = PathBuf::from(socket);
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-terminal-retry");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let data_ready = Arc::new(AtomicBool::new(false));
    let gate = data_ready.clone();
    std::thread::spawn(move || {
        for client in listener.incoming().map_while(Result::ok) {
            let server = server.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let _ = relay(client, &server, &gate);
            });
        }
    });
    let mut h = Harness::open(
        &format!("unix://{}", path.display()),
        &sessions.ids[0],
        840.,
        520.,
    );
    h.wait("a retriable error on the terminal stream", |m| {
        let id = m.view()?.active_window()?.focused_block_id.as_ref()?;
        matches!(
            m.panes.get(id)?.snapshot().status,
            PaneStatus::Reconnecting(_)
        )
        .then_some(())
    });
    data_ready.store(true, Ordering::Release);
    h.wait("the prompt without clicking Retry", |m| {
        let id = m.view()?.active_window()?.focused_block_id.as_ref()?;
        (m.panes.get(id)?.snapshot().status == PaneStatus::Live
            && !focused_text(m).trim().is_empty())
        .then_some(())
    });
}
