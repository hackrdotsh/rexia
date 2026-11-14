//! Open Onboarding Session… against a real Rex server named by REXIA_TEST_SERVER: the
//! walkthrough opens in its own session, rexia moves it between steps, and a step another
//! client chooses reaches rexia through the server's step_changed events.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::{host::HostModel, onboarding};
use serde_json::{Value, json};

/// The onboarding session and its walkthrough block, once rexia shows them.
fn walkthrough(m: &HostModel) -> Option<(String, String)> {
    let session = m.sessions.iter().find(|s| s.label == "Onboarding")?;
    if m.selected.as_deref() != Some(session.session_id.as_str()) {
        return None;
    }
    let window = m.view()?.active_window()?;
    let block = window
        .blocks()
        .find(|b| b.creator_name == onboarding::CREATOR)?
        .block_id
        .clone()?;
    Some((session.session_id.clone(), block))
}

#[test]
fn onboarding_opens_moves_and_follows_other_clients() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-onboarding-start");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    h.run("client.onboarding", Value::Null);
    let (session, block) = h.wait("the onboarding session", walkthrough);
    let step = |h: &mut Harness| {
        let block = block.clone();
        h.workspace(move |ws| ws.onboarding_step(&block))
    };
    let first = h.wait_ui("the server's answer", |ws| {
        ws.has_onboarding_answer(&block)
            .then(|| ws.onboarding_step(&block))
    });
    println!("opened at step {first:?}");
    assert_eq!(onboarding::index(&first), 0);
    h.screenshot("onboarding-welcome");

    // rexia moves it on.
    let window = h.window;
    let (s, b) = (session.clone(), block.clone());
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.set_onboarding_step(&s, &b, "look", cx))
            .unwrap()
    });
    h.wait_ui("the look step", |ws| {
        (ws.onboarding_step(&block) == "look").then_some(())
    });
    println!("rexia moved it to {:?}", step(&mut h));
    h.screenshot("onboarding-look");

    // Another client moves it; rexia follows the server's event.
    common::request(
        &control,
        &format!("{}.set_step", onboarding::CREATOR),
        json!({"session_id": session, "block_id": block, "args": {"step": "agents"}}),
    )
    .unwrap();
    h.wait_ui("the other client's step", |ws| {
        (ws.onboarding_step(&block) == "agents").then_some(())
    });
    println!("another client moved it to {:?}", step(&mut h));

    // Opening it again shows the same session.
    h.run("client.onboarding", Value::Null);
    let count = h.wait("the session list", |m| {
        Some(
            m.sessions
                .iter()
                .filter(|s| s.label == "Onboarding")
                .count(),
        )
    });
    println!("onboarding sessions after opening twice: {count}");
    assert_eq!(count, 1);
    let _ = common::request(&control, "session.destroy", json!({"session_id": session}));
    drop(sessions);
}
