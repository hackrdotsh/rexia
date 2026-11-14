//! Drives a host worker against a real Rex server. Set REXIA_TEST_SERVER to a disposable
//! server's address, such as unix:///tmp/rexia-test/rex.sock; the test creates its own
//! session there and destroys it at the end.
use rexia::rex::{
    client::Identity,
    endpoint::Endpoint,
    host::{Connection, HostCommand, HostHandle, HostModel, PaneStatus},
};
use serde_json::json;
use std::time::{Duration, Instant};

fn wait_for<T>(host: &HostHandle, what: &str, mut check: impl FnMut(&HostModel) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(value) = check(&host.snapshot()) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            host.snapshot().connection
        );
        let _ = host.wake.recv_timeout(Duration::from_millis(100));
    }
}

fn pane_text(host: &HostHandle, block: &str) -> String {
    host.pane(block)
        .and_then(|p| p.snapshot().frame)
        .map(|f| {
            (0..f.rows as usize)
                .map(|r| f.line_text(r))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Destroys the test's session even when an assertion fails partway.
struct Cleanup<'a> {
    host: &'a HostHandle,
    session: String,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.host.send(HostCommand::Call {
            method: "session.destroy".into(),
            payload: json!({"session_id": self.session}),
            reply: Some(reply),
        });
        let _ = answer.blocking_recv();
    }
}

#[test]
fn host_streams_types_splits_and_cleans_up() {
    let Ok(address) = std::env::var("REXIA_TEST_SERVER") else {
        eprintln!("skipped: set REXIA_TEST_SERVER to a disposable Rex server");
        return;
    };
    let host = HostHandle::spawn(
        Endpoint::parse(&address).unwrap(),
        Identity::default(),
        None,
    );
    wait_for(&host, "connection", |m| {
        (m.connection == Connection::Connected).then_some(())
    });
    let revision = host.snapshot().api_revision;
    println!("connected at API revision {revision}");

    let (reply, answer) = tokio::sync::oneshot::channel();
    host.send(HostCommand::CreateSession {
        label: Some("rexia-live-test".into()),
        reply: Some(reply),
    });
    let created = answer.blocking_recv().unwrap().unwrap();
    let session = created["session_id"].as_str().unwrap().to_owned();
    let cleanup = Cleanup {
        host: &host,
        session: session.clone(),
    };
    let block = wait_for(&host, "the new session's terminal", |m| {
        (m.selected.as_deref() == Some(&session)).then_some(())?;
        let window = m.view()?.active_window()?;
        window.panes().first().map(|p| p.block_id.clone())
    });
    wait_for(&host, "a live frame", |m| {
        let pane = m.panes.get(&block)?.snapshot();
        (pane.status == PaneStatus::Live && pane.frame.is_some()).then_some(())
    });
    println!("streaming {block}");

    host.send(HostCommand::Resize {
        session_id: session.clone(),
        block_id: block.clone(),
        cols: 90,
        rows: 20,
        cell_width: 9,
        cell_height: 18,
    });
    host.send(HostCommand::Input {
        session_id: session.clone(),
        block_id: block.clone(),
        method: "block.event",
        payload: json!({"session_id": session, "block_id": block,
            "event": {"type": "paste", "payload": {"text": "echo rexia-live-output\r"}}}),
        claim: true,
    });
    // Only the command's output is a line of exactly these words; the typed line has `echo`.
    wait_for(&host, "the echoed output", |_| {
        let text = pane_text(&host, &block);
        text.lines()
            .any(|line| line.trim() == "rexia-live-output")
            .then_some(())
    });
    let frame = host.pane(&block).unwrap().snapshot().frame.unwrap();
    assert_eq!(
        (frame.cols, frame.rows),
        (90, 20),
        "the server applied our size"
    );
    assert!(
        host.pane(&block).unwrap().snapshot().owner,
        "typing claimed the block"
    );
    println!(
        "typed and read back:\n{}",
        pane_text(&host, &block).trim_end()
    );

    host.call(
        "session.new_split",
        json!({"session_id": session, "anchor_id": block, "direction": "horizontal", "side": "after",
            "layout": rexia::rex::host::shell_layout(), "focus": true}),
    );
    let panes = wait_for(&host, "the split", |m| {
        let window = m.view()?.active_window()?;
        (window.panes().len() == 2 && window.dividers().len() == 1).then(|| window.panes())
    });
    wait_for(&host, "both panes streaming", |m| {
        panes
            .iter()
            .all(|p| {
                m.panes
                    .get(&p.block_id)
                    .is_some_and(|h| h.snapshot().frame.is_some())
            })
            .then_some(())
    });
    println!("split into {} panes", panes.len());

    // Programs report status over OSC 7501; the server relays it as block events. A record
    // lasts as long as the process that reported it, so the reporter stays alive.
    host.send(HostCommand::Input {
        session_id: session.clone(),
        block_id: block.clone(),
        method: "block.event",
        payload: json!({"session_id": session, "block_id": block,
            "event": {"type": "paste", "payload": {"text": "sh -c 'printf \"\\033]7501;state=blocked:kind=question:app=rexia-test\\033\\\\\\\\\"; sleep 30'\r"}}}),
        claim: true,
    });
    let badge = wait_for(&host, "program status", |m| {
        m.blocks.get(&block)?.status().map(|r| r.badge())
    });
    println!("program status badge: {badge}");
    assert_eq!(badge, "Question");

    drop(cleanup);
    wait_for(&host, "the session to go away", |m| {
        m.session(&session).is_none().then_some(())
    });
    println!("destroyed {session}");
}
