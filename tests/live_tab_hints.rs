//! Holding the Go to Tab modifier shows each tab's key, against a real Rex server named by
//! REXIA_TEST_SERVER: not at once (Alt combinations typed in a terminal don't flash them),
//! then on every tab; pressing a key with it goes there and hides them; letting go hides
//! them; another modifier shows nothing.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Capslock, Modifiers, ModifiersChangedEvent, PlatformInput};
use serde_json::Value;
use std::time::Duration;

fn hold(h: &mut Harness, modifiers: Modifiers) {
    h.input(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers,
        capslock: Capslock::default(),
    }));
}

fn wait(h: &mut Harness, ms: u64) {
    h.cx.advance_clock(Duration::from_millis(ms));
    h.pump();
}

fn hints(h: &mut Harness) -> Vec<Option<String>> {
    h.workspace(|ws| ws.tab_hints())
}

#[test]
fn holding_the_tab_modifier_shows_tab_keys() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-tab-hints");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 760.);
    h.wait("a prompt and the keymap", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
    });
    for _ in 0..2 {
        h.run("client.tab.new", Value::Null);
    }
    h.wait("three tabs", |m| {
        (m.view()?.windows.len() == 3).then_some(())
    });
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };

    // Held: nothing at first, then a key on every tab.
    hold(&mut h, alt);
    wait(&mut h, 200);
    let early = hints(&mut h);
    wait(&mut h, 300);
    let shown = hints(&mut h);
    println!("holding Alt: after 200 ms {early:?}, after 500 ms {shown:?}");
    assert!(early.is_empty());
    assert_eq!(
        shown,
        [
            Some("Alt+1".into()),
            Some("Alt+2".into()),
            Some("Alt+3".into())
        ]
    );
    h.screenshot("tab-hints");

    // Alt+2 goes to the second tab, and the hints go.
    let second = h.wait("the second tab", |m| {
        Some(m.view()?.windows.get(1)?.id().to_owned())
    });
    h.press("alt-2");
    let selected = h.wait("the second tab selected", |m| {
        let id = m.view()?.active_window_id.clone()?;
        (id == second).then_some(id)
    });
    println!("Alt+2: selected {selected}, hints {:?}", hints(&mut h));
    assert!(hints(&mut h).is_empty());

    // Held again, then let go.
    hold(&mut h, Modifiers::default());
    hold(&mut h, alt);
    wait(&mut h, 500);
    assert_eq!(hints(&mut h).len(), 3);
    hold(&mut h, Modifiers::default());
    println!("let go: hints {:?}", hints(&mut h));
    assert!(hints(&mut h).is_empty());

    // Another modifier shows nothing.
    hold(
        &mut h,
        Modifiers {
            control: true,
            ..Default::default()
        },
    );
    wait(&mut h, 600);
    println!("holding Ctrl: hints {:?}", hints(&mut h));
    assert!(hints(&mut h).is_empty());
    drop(sessions);
}
