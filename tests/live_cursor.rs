//! Cursor style, force style and blinking, against a real Rex server named by
//! REXIA_TEST_SERVER. Programs set the cursor with DECSCUSR (CSI n SP q).
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{rex::stream::CursorShape, settings::BlinkSpeed, settings::Settings};
use std::time::{Duration, Instant};

type Drawn = (CursorShape, bool, bool);

fn cursor(h: &mut Harness) -> Option<Drawn> {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| window.read(cx).unwrap().focused_cursor(cx))
}

fn wait_cursor(h: &mut Harness, what: &str, check: impl Fn(Drawn) -> bool) -> Drawn {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let drawn = cursor(h);
        if let Some(drawn) = drawn.filter(|d| check(*d)) {
            return drawn;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {drawn:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

/// Run a command in the focused pane's shell and wait for its prompt to come back.
fn run(h: &mut Harness, command: &str, marker: &str) {
    // The marker is printed in two halves, so only the output matches it: a long prompt
    // can wrap the typed line, and the shell may redraw over the wrapped part.
    let (first, second) = marker.split_once('-').expect("a marker with a hyphen");
    h.type_text(&format!("{command}; printf '%s-%s\\n' {first} {second}"));
    h.press("enter");
    let marker = marker.to_owned();
    h.wait("the command", |m| {
        focused_text(m).contains(&marker).then_some(())
    });
}

fn blink(h: &mut Harness) -> Drawn {
    let window = h.window;
    h.cx.update(|cx| window.update(cx, |ws, _, cx| ws.blink_focused(cx)).unwrap());
    cursor(h).unwrap()
}

#[test]
fn cursor_style_force_and_blink() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-cursor");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let start = wait_cursor(&mut h, "the default block", |d| d.0 == CursorShape::Block);
    println!("default: {start:?}");

    // The chosen style reaches a cursor the program left at the default.
    settings(&mut h, |s| s.terminal.cursor_style = CursorShape::Bar);
    let bar = wait_cursor(&mut h, "the bar default", |d| d.0 == CursorShape::Bar);
    println!("style bar: {bar:?}");
    assert_eq!(
        bar,
        (CursorShape::Bar, false, true),
        "a steady default cursor"
    );

    // A program's blinking block wins over the default, and blinks.
    run(&mut h, r"printf '\033[1 q'", "asked-blink");
    let asked = wait_cursor(&mut h, "the program's blinking block", |d| {
        d.0 == CursorShape::Block
    });
    println!("program asked for a blinking block: {asked:?}");
    assert_eq!(asked, (CursorShape::Block, true, true));
    // Typing holds the cursor solid for one interval; then it blinks.
    std::thread::sleep(Duration::from_millis(700));
    let off = blink(&mut h);
    let on = blink(&mut h);
    println!("blink: {off:?} then {on:?}");
    assert!(!off.2 && on.2, "the cursor hides, then shows");

    // Force style draws the chosen shape whatever the program asked for.
    settings(&mut h, |s| {
        s.terminal.cursor_style = CursorShape::Underline;
        s.terminal.force_cursor_style = true;
    });
    let forced = wait_cursor(&mut h, "the forced underline", |d| {
        d.0 == CursorShape::Underline
    });
    println!("forced underline: {forced:?}");

    // Blink speed Off keeps every cursor steady.
    settings(&mut h, |s| s.terminal.cursor_blink = BlinkSpeed::Off);
    let steady = wait_cursor(&mut h, "a steady cursor", |d| !d.1);
    assert!(blink(&mut h).2, "an Off cursor never hides");
    println!("blink off: {steady:?}");

    // Without force, a program's reset (CSI 0 q) returns to the chosen default.
    settings(&mut h, |s| {
        s.terminal.force_cursor_style = false;
        s.terminal.cursor_blink = BlinkSpeed::Normal;
    });
    run(&mut h, r"printf '\033[0 q'", "asked-reset");
    let reset = wait_cursor(&mut h, "the underline default", |d| {
        d == (CursorShape::Underline, false, true)
    });
    println!("program reset: {reset:?}");
    h.screenshot("cursor-underline");
    drop(sessions);
}
