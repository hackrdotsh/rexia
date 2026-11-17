//! The pointer hides while typing in a terminal, against a real Rex server named by
//! REXIA_TEST_SERVER: every key the terminal takes (letters, Return, Backspace, arrows,
//! Ctrl combinations) asks GPUI to hide it until the mouse moves, and a shortcut rexia
//! takes for itself doesn't. GPUI's test platform has no pointer, so this counts the asks.
mod common;

use common::{Harness, Sessions, focused_text};

fn hides(h: &mut Harness) -> usize {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).pointer_hides()
    })
}

#[test]
fn typing_in_a_terminal_hides_the_pointer() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-hide-pointer");
    let mut h = Harness::open(&address, &sessions.ids[0], 1000., 640.);
    h.wait("a prompt and the keymap", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
    });
    let start = hides(&mut h);
    for key in ["a", "enter", "backspace", "left", "ctrl-c"] {
        let before = hides(&mut h);
        h.press(key);
        let after = hides(&mut h);
        println!("{key}: {} ask(s) to hide the pointer", after - before);
        assert_eq!(after, before + 1, "{key} should hide the pointer");
    }

    // Alt+1 is rexia's Go to Tab, here to the tab already shown: it never reaches the
    // terminal, so it leaves the pointer be.
    let before = hides(&mut h);
    h.press("alt-1");
    let after = hides(&mut h);
    println!("alt-1 (Go to Tab): {} ask(s)", after - before);
    assert_eq!(after, before);
    println!("5 keys typed, {} asks in all", after - start);
    drop(sessions);
}
