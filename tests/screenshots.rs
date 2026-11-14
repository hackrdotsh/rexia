//! Renders rexia's views headlessly against a real Rex server and saves PNGs, so the
//! interface can be checked without a display. Set REXIA_TEST_SERVER to a disposable
//! server; images go to REXIA_SHOT_DIR (default target/screenshots).
use gpui::{AppContext as _, HeadlessAppContext, px, size};
use rexia::{
    noctalia,
    rex::{
        client::Identity,
        endpoint::Endpoint,
        host::{Connection, HostCommand, HostHandle, HostModel},
    },
    ui::{fonts, terminal::TerminalPane, theme::Theme},
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn wait_for<T>(host: &HostHandle, what: &str, mut check: impl FnMut(&HostModel) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(value) = check(&host.snapshot()) {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        let _ = host.wake.recv_timeout(Duration::from_millis(50));
    }
}

fn shot_dir() -> PathBuf {
    let dir = std::env::var_os("REXIA_SHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn headless() -> HeadlessAppContext {
    HeadlessAppContext::with_platform(
        Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
        Arc::new(()),
        || gpui_platform::current_headless_renderer(),
    )
}

/// Destroys the test's session even when an assertion fails partway.
struct Cleanup {
    host: HostHandle,
    session: String,
}

impl Drop for Cleanup {
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
fn terminal_pane_renders_a_live_session() {
    let Ok(address) = std::env::var("REXIA_TEST_SERVER") else {
        eprintln!("skipped: set REXIA_TEST_SERVER to a disposable Rex server");
        return;
    };
    let theme = Theme::from_noctalia(&noctalia::load());
    let host = HostHandle::spawn(
        Endpoint::parse(&address).unwrap(),
        Identity::default(),
        None,
    );
    host.send(HostCommand::TerminalTheme(Some(theme.terminal_theme())));
    wait_for(&host, "connection", |m| {
        (m.connection == Connection::Connected).then_some(())
    });
    let (reply, answer) = tokio::sync::oneshot::channel();
    host.send(HostCommand::CreateSession {
        label: Some("rexia-shot".into()),
        reply: Some(reply),
    });
    let session = answer.blocking_recv().unwrap().unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let _cleanup = Cleanup {
        host: host.clone(),
        session: session.clone(),
    };
    let block = wait_for(&host, "terminal", |m| {
        m.view()?
            .active_window()?
            .panes()
            .first()
            .map(|p| p.block_id.clone())
    });

    let mut cx = headless();
    cx.update(|cx| cx.set_global(theme.clone()));
    let font = fonts::default_terminal_font();
    println!("terminal font: {} {}pt", font.family, font.size);
    let window = cx
        .open_window(size(px(960.), px(440.)), |_, cx| {
            cx.new(|cx| TerminalPane::new(host.clone(), session.clone(), block.clone(), font, cx))
        })
        .unwrap();
    cx.run_until_parked();

    let script = concat!(
        "printf '\\033[1mrexia\\033[0m on \\033[38;5;2mRex\\033[0m · ",
        "\\033[31mred\\033[0m \\033[32mgreen\\033[0m \\033[33myellow\\033[0m \\033[34mblue\\033[0m ",
        "\\033[35mmagenta\\033[0m \\033[36mcyan\\033[0m \\033[4munderline\\033[0m \\033[7minverse\\033[0m 界\\n'"
    );
    host.send(HostCommand::Input {
        session_id: session.clone(),
        block_id: block.clone(),
        method: "block.event",
        payload: json!({"session_id": session, "block_id": block,
            "event": {"type": "paste", "payload": {"text": script}}}),
        claim: true,
    });
    host.send(HostCommand::Input {
        session_id: session.clone(),
        block_id: block.clone(),
        method: "client.key",
        payload: json!({"session_id": session, "block_id": block,
            "key": {"key": "enter", "mods": "", "text": "\r", "unshifted_codepoint": 13, "action": "press"}}),
        claim: true,
    });
    let frame = wait_for(&host, "rendered output", |m| {
        let pane = m.panes.get(&block)?.snapshot();
        let frame = pane.frame?;
        let text: Vec<String> = (0..frame.rows as usize)
            .map(|r| frame.line_text(r))
            .collect();
        text.iter()
            .any(|l| l.starts_with("rexia on Rex"))
            .then_some(frame)
    });
    println!(
        "server grid after fitting the pane: {}x{}",
        frame.cols, frame.rows
    );
    // Let the last frames arrive, then draw them.
    std::thread::sleep(Duration::from_millis(300));
    cx.update_window(window.into(), |_, window, _| window.refresh())
        .unwrap();
    cx.run_until_parked();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let path = shot_dir().join("terminal-pane.png");
    image.save(&path).unwrap();
    println!("saved {}", path.display());
    assert!(frame.cols > 80, "the pane asked for its own width");

    drop(_cleanup);
    wait_for(&host, "cleanup", |m| {
        m.session(&session).is_none().then_some(())
    });
}
