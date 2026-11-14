//! Terminal › Resizing against a real Rex server named by REXIA_TEST_SERVER. A pane is
//! held from asking the server for a new size (as moving panes are), then narrowed by a
//! split: with smooth resizing its text reflows here anyway; without it, it waits until
//! the hold lapses and the server is asked.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    rex::host::HostHandle,
    settings::{AnimationStyle, Settings},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Columns locally and as asked of the server.
type Sizes = (Option<u16>, Option<u16>);

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

/// For `block`: the columns its local terminal has, and the columns asked of the server.
fn sizes(h: &mut Harness, block: &str) -> Sizes {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        let ws = window.read(cx).unwrap();
        let local = ws
            .model()
            .panes
            .get(block)
            .and_then(|p| p.snapshot().frame)
            .map(|f| f.cols);
        let asked = ws
            .terminal_for(block)
            .and_then(|p| p.read(cx).requested_size())
            .map(|(cols, _)| cols);
        (local, asked)
    })
}

/// Wait until `check` accepts `block`'s sizes.
fn sizes_until(h: &mut Harness, block: &str, what: &str, check: impl Fn(Sizes) -> bool) -> Sizes {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let now = sizes(h, block);
        if check(now) {
            return now;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {now:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Focus `block`, hold its server size for `hold`, split it, and return its sizes before
/// and a second later.
fn held_split(
    h: &mut Harness,
    session: &str,
    control: &HostHandle,
    block: &str,
    hold: Duration,
) -> (Sizes, Sizes) {
    common::request(
        control,
        "session.focus_block",
        json!({"session_id": session, "block_id": block}),
    )
    .unwrap();
    h.wait("the pane focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(block)).then_some(())
    });
    let before = sizes_until(h, block, "a settled pane", |s| s.0.is_some() && s.0 == s.1);
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.hold_pane_size(block, hold, cx))
            .unwrap()
    });
    h.run("pane.split.right", Value::Null);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        sizes(h, block);
        std::thread::sleep(Duration::from_millis(20));
    }
    let held = sizes(h, block);
    println!("before {before:?}, a second into the hold {held:?}");
    (before, held)
}

#[test]
fn smooth_resizing_reflows_before_the_server() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-resizing");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    let first = h.wait("a prompt", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        (!focused_text(m).trim().is_empty()).then_some(block)
    });
    // Panes jump to their new sizes, so the narrower size is drawn at once.
    settings(&mut h, |s| s.interface.animation = AnimationStyle::Off);

    // Smooth resizing: the text is narrower than anything the server was asked for.
    let hold = Duration::from_secs(4);
    let (before, held) = held_split(&mut h, &session, &control, &first, hold);
    assert_eq!(held.1, before.1, "the server wasn't asked for the new size");
    assert!(held.0 < before.0, "the text reflowed here");

    // Off: held, the text keeps the size the server knows, then narrows with it.
    settings(&mut h, |s| s.terminal.smooth_resizing = false);
    let (before, held) = held_split(&mut h, &session, &control, &first, hold);
    assert_eq!(
        held, before,
        "without smooth resizing the text waits for the server"
    );
    let after = sizes_until(&mut h, &first, "the pane to narrow", |s| {
        s.0 < before.0 && s.0 == s.1
    });
    println!("after the hold: {after:?}");
    drop(sessions);
}
