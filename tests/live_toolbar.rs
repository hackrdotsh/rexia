//! Toolbar layouts and visibility, against a real Rex server named by REXIA_TEST_SERVER.
//! Where the panes are drawn shows where the toolbar and sidebar went.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{AnyWindowHandle, Modifiers, MouseMoveEvent, PlatformInput, point, px};
use rexia::settings::{PaneHeaders, Settings, ToolbarBehavior, ToolbarLayout};
use serde_json::Value;

const WIDTH: f32 = 1280.;
const HEIGHT: f32 = 800.;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

/// The panes' area once it has settled after a change: (left, top, right).
fn panes(h: &mut Harness, what: &str, check: impl Fn(f32, f32, f32) -> bool) -> (f32, f32, f32) {
    h.wait_ui(what, |ws| {
        let area = ws.pane_area();
        let (x, y) = (f32::from(area.origin.x), f32::from(area.origin.y));
        let right = x + f32::from(area.size.width);
        check(x, y, right).then_some((x, y, right))
    })
}

fn pointer(h: &mut Harness, y: f32) {
    let handle: AnyWindowHandle = h.window.into();
    h.cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: point(px(WIDTH / 2.), px(y)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    })
    .unwrap();
    h.pump();
}

fn toolbar(h: &mut Harness) -> bool {
    h.workspace(|ws| ws.toolbar_visible())
}

#[test]
fn toolbar_layouts_behaviors_and_reveals() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-toolbar");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, WIDTH, HEIGHT);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Above: the tab bar over the panes, the sidebar to their left.
    let above = panes(&mut h, "panes under the tab bar", |x, y, _| {
        y >= 48. && x >= 260.
    });
    println!("above: panes at {above:?}");

    // Left side: tabs move into the sidebar, so the panes reach the window's top.
    settings(&mut h, |s| s.interface.toolbar_layout = ToolbarLayout::Left);
    let left = panes(&mut h, "panes beside a left toolbar", |x, y, _| {
        y < 10. && x >= 260.
    });
    println!("left: panes at {left:?}");
    h.screenshot("toolbar-left");

    // Right side: the sidebar and its toolbar move to the right edge.
    settings(&mut h, |s| {
        s.interface.toolbar_layout = ToolbarLayout::Right
    });
    let right = panes(&mut h, "panes beside a right toolbar", |x, y, r| {
        y < 10. && x < 10. && r <= WIDTH - 260.
    });
    println!("right: panes at {right:?}");
    h.screenshot("toolbar-right");

    // Toggle Toolbar Layout goes to Above and back to the side last used.
    h.press("ctrl-shift-s");
    panes(&mut h, "tabs above again", |_, y, _| y >= 48.);
    h.press("ctrl-shift-s");
    let back = h.workspace(|ws| ws.settings.interface.toolbar_layout);
    println!("toggle twice -> {back:?}");
    assert_eq!(back, ToolbarLayout::Right);

    // Show on hover: hidden, and the panes keep the whole height either way.
    settings(&mut h, |s| {
        s.interface.toolbar_layout = ToolbarLayout::Above;
        s.interface.toolbar_behavior = ToolbarBehavior::Hover;
    });
    panes(&mut h, "panes at the top", |_, y, _| y < 10.);
    assert!(!toolbar(&mut h));
    pointer(&mut h, 2.);
    assert!(toolbar(&mut h), "the top edge reveals it");
    h.screenshot("toolbar-hover");
    panes(&mut h, "panes still at the top", |_, y, _| y < 10.);
    pointer(&mut h, 30.);
    assert!(toolbar(&mut h), "it stays while the pointer is on it");
    pointer(&mut h, 400.);
    assert!(!toolbar(&mut h), "leaving it hides it");
    println!("hover: hidden, shown at the top edge, hidden again below it");

    // Hidden: only Reveal Toolbar shows it, and it holds for a moment wherever the pointer is.
    settings(&mut h, |s| {
        s.interface.toolbar_behavior = ToolbarBehavior::Hidden
    });
    pointer(&mut h, 2.);
    assert!(
        !toolbar(&mut h),
        "the top edge doesn't reveal a hidden toolbar"
    );
    h.run("client.toolbar.reveal", Value::Null);
    assert!(toolbar(&mut h));
    pointer(&mut h, 400.);
    assert!(toolbar(&mut h), "a revealed toolbar holds for a moment");
    h.press("ctrl-alt-shift-t");
    assert!(!toolbar(&mut h), "revealing again hides it");
    println!("hidden: Reveal Toolbar shows it, and again hides it");

    // Pane headers: a hidden header shows for the focused pane on Reveal Pane Header.
    settings(&mut h, |s| {
        s.interface.toolbar_behavior = ToolbarBehavior::Always;
        s.interface.pane_headers = PaneHeaders::Hidden;
    });
    let block = h.wait("the focused pane", |m| {
        m.view()?.active_window()?.focused_block_id.clone()
    });
    assert!(!h.workspace(|ws| ws.header_shown(&block)));
    h.press("ctrl-alt-shift-h");
    assert!(h.workspace(|ws| ws.header_shown(&block)));
    h.screenshot("pane-header-revealed");
    println!("pane header: hidden, then revealed for {block}");
    drop(sessions);
}
