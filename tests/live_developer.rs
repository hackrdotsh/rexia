//! Settings › Developer: the local API address and web client origins. The origins are
//! proven on a server this test starts with REXIA_TEST_REX and the arguments Restart
//! Server would use: a browser handshake from an allowed origin is accepted, others refused.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::SettingsSection;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    time::Duration,
};

/// The status line a WebSocket handshake from `origin` gets.
fn handshake(port: u16, origin: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /api/control HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: rex.control.v1\r\n\
         Origin: {origin}\r\n\r\n"
    )
    .unwrap();
    let mut reply = [0u8; 256];
    let n = stream.read(&mut reply).unwrap_or(0);
    String::from_utf8_lossy(&reply[..n])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn local_api_and_web_client_origins() {
    let (Some(address), Ok(rex)) = (common::server(), std::env::var("REXIA_TEST_REX")) else {
        eprintln!("skipped: needs REXIA_TEST_SERVER and REXIA_TEST_REX");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-developer");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.run("client.settings", json!({}));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.show_settings_section(SettingsSection::Developer, window, cx)
            })
            .unwrap()
    });
    let url = h.workspace(|ws| ws.local_api_url());
    println!("local API: {url:?}");
    assert_eq!(url.as_deref(), Some(address.as_str()));

    // Origins: a wildcard subdomain goes in; a path and a duplicate are refused.
    let added = h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                [
                    ws.add_origin("https://*.example.com", cx),
                    ws.add_origin("https://example.com/app", cx),
                    ws.add_origin("HTTPS://*.example.com/", cx),
                ]
            })
            .unwrap()
    });
    println!("added: {added:?}");
    assert!(added[0].is_ok() && added[1].is_err());
    assert_eq!(
        added[2],
        Err("https://*.example.com is already allowed".into())
    );
    h.pump();
    h.screenshot("settings-developer");
    let args = h.workspace(|ws| ws.restart_args());
    println!("Restart Server runs: rex {}", args.join(" "));
    assert!(args.ends_with(&[
        "--websocket-origin-pattern".into(),
        "https://*.example.com".into()
    ]));

    // A server started with those origins takes browsers from them, and only them.
    let dir = tempfile::tempdir().unwrap();
    let socket = format!("unix://{}", dir.path().join("rex.sock").display());
    let origins: Vec<String> = args[4..].to_vec();
    let mut child = Command::new(&rex)
        .args([
            "server",
            "run",
            "--listen",
            &socket,
            "--extra-listen",
            "127.0.0.1:0",
            "--allow-anonymous",
        ])
        .args(&origins)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .arg("--restore=false")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let probe = common::connect(&socket);
    let status = common::request(&probe, "server.status", json!({})).unwrap();
    let port: u16 = status["listeners"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["transport"] == "tcp")
        .and_then(|l| l["addresses"][0].as_str())
        .and_then(|a| a.rsplit(':').next()?.parse().ok())
        .unwrap();
    let allowed = handshake(port, "https://app.example.com");
    let refused = handshake(port, "https://evil.test");
    println!("from app.example.com: {allowed}; from evil.test: {refused}");
    let _ = child.kill();
    let _ = child.wait();
    assert!(allowed.contains("101"), "{allowed}");
    assert!(!refused.contains("101"), "{refused}");

    // Removing it takes it out of the next restart.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.remove_origin("https://*.example.com", cx)
            })
            .unwrap()
    });
    assert!(
        !h.workspace(|ws| ws.restart_args())
            .contains(&"--websocket-origin-pattern".into())
    );
    drop(sessions);
}
