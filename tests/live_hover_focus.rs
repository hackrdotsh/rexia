//! Focus follows mouse, against a real Rex server named by REXIA_TEST_SERVER: moving the
//! pointer onto another pane focuses it, and what you type then goes to that pane, not the
//! one focused before. Back and forth, both ways.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Modifiers, MouseMoveEvent, PlatformInput, point};
use rexia::rex::host::HostModel;
use serde_json::{Value, json};

fn text(model: &HostModel, block: &str) -> String {
    model
        .panes
        .get(block)
        .and_then(|p| p.snapshot().frame)
        .map(|f| {
            (0..f.rows as usize)
                .map(|r| f.line_text(r))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn hover(h: &mut Harness, block: &str) {
    let bounds = h
        .workspace(|ws| ws.pane_bounds(block))
        .expect("the pane on screen");
    let center = bounds.center();
    for dx in [-20., -10., 0.] {
        h.input(PlatformInput::MouseMove(MouseMoveEvent {
            position: point(center.x + gpui::px(dx), center.y),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }));
    }
    let wanted = block.to_owned();
    h.wait("the hovered pane focused", |m| {
        let focused = m.view()?.active_window()?.focused_block_id.clone()?;
        (focused == wanted).then_some(())
    });
    h.pump();
}

/// Type a marker and see which pane it lands in.
fn type_into(h: &mut Harness, marker: &str, expected: &str, other: &str) {
    h.type_text(&format!("echo {marker}"));
    let (expected, other, marker) = (expected.to_owned(), other.to_owned(), marker.to_owned());
    let landed = h.wait(&format!("{marker} typed"), |m| {
        if text(m, &expected).contains(&marker) {
            Some(true)
        } else if text(m, &other).contains(&marker) {
            Some(false)
        } else {
            None
        }
    });
    println!("{marker}: landed in the hovered pane: {landed}");
    assert!(landed, "{marker} went to the pane focused before the hover");
    h.press("ctrl-c");
}

#[test]
fn typing_goes_to_the_hovered_pane() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-hover-focus");
    let mut settings = common::test_settings(&address, &sessions.ids[0]);
    settings.interface.focus_follows_mouse = true;
    let mut h = Harness::open_with(settings, 1100., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let top = h.wait("the first pane", |m| {
        m.view()?.active_window()?.focused_block_id.clone()
    });
    h.run("pane.split.down", Value::Null);
    let bottom = h.wait("the split", |m| {
        let window = m.view()?.active_window()?;
        let focused = window.focused_block_id.clone()?;
        (window.panes().len() == 2 && focused != top).then_some(focused)
    });
    // Focus the top pane by keyboard, as a starting point.
    h.call_from(
        &control,
        "session.focus_block",
        json!({"session_id": sessions.ids[0], "block_id": top}),
    )
    .unwrap();
    let wanted = top.clone();
    h.wait("the top pane focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_ref()? == &wanted).then_some(())
    });
    h.wait("a prompt in each pane", |m| {
        (!text(m, &top).trim().is_empty() && !text(m, &bottom).trim().is_empty()).then_some(())
    });

    hover(&mut h, &bottom);
    type_into(&mut h, "HOVERED-BOTTOM", &bottom, &top);
    hover(&mut h, &top);
    type_into(&mut h, "HOVERED-TOP", &top, &bottom);
    hover(&mut h, &bottom);
    type_into(&mut h, "BOTTOM-AGAIN", &bottom, &top);
    drop(sessions);
}
