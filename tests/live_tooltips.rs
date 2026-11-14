//! Tooltips on buttons, against a real Rex server named by REXIA_TEST_SERVER: resting the
//! pointer on a toolbar button shows its name and the keys bound to it, and a changed
//! shortcut shows in the tooltip.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Modifiers, MouseMoveEvent, PlatformInput, point, px};
use rexia::ui::components::ShownTooltip;
use serde_json::Value;
use std::time::Duration;

const WIDTH: f32 = 1200.;
const HEIGHT: f32 = 760.;

fn shown(h: &mut Harness) -> Option<String> {
    h.cx.update(|cx| cx.try_global::<ShownTooltip>().and_then(|t| t.0.clone()))
}

/// Rest the pointer at `x`, `y` and wait for a tooltip to appear there.
fn hover(h: &mut Harness, x: f32, y: f32) -> String {
    h.cx.update(|cx| cx.set_global(ShownTooltip(None)));
    // Away first, so the button sees the pointer arrive.
    for (x, y) in [(WIDTH / 2., HEIGHT / 2.), (x, y)] {
        h.input(PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }));
    }
    // GPUI's test clock only moves when told to: not yet at 300 ms, shown by 600 ms.
    h.cx.advance_clock(Duration::from_millis(300));
    h.pump();
    assert_eq!(shown(h), None, "tooltips wait before showing");
    h.cx.advance_clock(Duration::from_millis(300));
    h.pump();
    shown(h).unwrap_or_else(|| panic!("no tooltip at {x}, {y}"))
}

#[test]
fn buttons_show_their_names_and_shortcuts() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-tooltips");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, WIDTH, HEIGHT);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // The toolbar: the sidebar toggle at its left, the split buttons at its right.
    let sidebar = hover(&mut h, 25., 24.);
    println!("sidebar toggle: {sidebar:?}");
    assert_eq!(sidebar, "Toggle Sidebar  Ctrl+Shift+B");
    let down = hover(&mut h, WIDTH - 25., 24.);
    println!("split down: {down:?}");
    assert_eq!(down, "Split Pane Down  Ctrl+Shift+E");
    let right = hover(&mut h, WIDTH - 63., 24.);
    println!("split right: {right:?}");
    assert_eq!(right, "Split Pane Right  Ctrl+Shift+O");
    h.screenshot("tooltip");

    // A new shortcut shows in the tooltip once the server takes it.
    h.run("client.shortcuts.edit", Value::Null);
    h.type_text("split pane right");
    h.press("enter");
    assert_eq!(
        h.workspace(|ws| ws.recording_shortcut()).as_deref(),
        Some("pane.split.right")
    );
    h.press("ctrl-alt-k");
    h.wait_ui("the new binding", |ws| {
        ws.keys_for_action("pane.split.right")
            .filter(|k| k.contains("ctrl+alt+k"))
    });
    let right = hover(&mut h, WIDTH - 63., 24.);
    println!("split right after Change Shortcut: {right:?}");
    assert_eq!(right, "Split Pane Right  Ctrl+Alt+K");

    // Without a shortcut, only the name shows.
    h.run("client.shortcuts.edit", Value::Null);
    h.type_text("split pane right");
    h.press("enter");
    h.press("backspace");
    h.wait_ui("the binding removed", |ws| {
        ws.keys_for_action("pane.split.right")
            .is_none()
            .then_some(())
    });
    let right = hover(&mut h, WIDTH - 63., 24.);
    println!("split right without a shortcut: {right:?}");
    assert_eq!(right, "Split Pane Right");
    drop(sessions);
}
