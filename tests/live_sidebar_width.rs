//! Resizing the sidebar against a real Rex server named by REXIA_TEST_SERVER: dragging its
//! edge widens it and moves the panes over, the width is kept when the drag ends, it stops
//! at its limits, a double-click puts it back, and on the right side dragging left widens it.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point, px,
};
use rexia::settings::ToolbarLayout;

fn down(h: &mut Harness, x: f32, clicks: usize) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: point(px(x), px(400.)),
        modifiers: Modifiers::default(),
        click_count: clicks,
        first_mouse: false,
    }));
}

fn drag_to(h: &mut Harness, x: f32) {
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: point(px(x), px(400.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    }));
}

fn up(h: &mut Harness, x: f32) {
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: point(px(x), px(400.)),
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
}

/// The left edge of the focused pane's text, in window pixels.
fn pane_left(h: &mut Harness) -> f32 {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        f32::from(pane.read(cx).cell_point(0, 0).unwrap().x)
    })
}

fn saved(h: &mut Harness) -> f32 {
    h.workspace(|ws| ws.settings.interface.sidebar_width)
}

#[test]
fn sidebar_resizes_by_its_edge_and_remembers() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-sidebar-width");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    assert_eq!(h.workspace(|ws| ws.sidebar_width()), 260.);
    let before = pane_left(&mut h);

    // Drag the edge 100 pixels right: wider while dragging, kept when let go.
    down(&mut h, 257., 1);
    drag_to(&mut h, 307.);
    drag_to(&mut h, 357.);
    let dragging = h.workspace(|ws| ws.sidebar_width());
    assert_eq!(saved(&mut h), 260., "saved only when let go");
    up(&mut h, 357.);
    let after = pane_left(&mut h);
    println!(
        "dragged: {dragging} while held, {} kept; panes moved {before} -> {after}",
        saved(&mut h)
    );
    assert_eq!((dragging, saved(&mut h)), (360., 360.));
    assert!((after - before - 100.).abs() < 1.5, "the panes moved over");
    h.screenshot("sidebar-wide");

    // It stops at its widest.
    down(&mut h, 357., 1);
    drag_to(&mut h, 1000.);
    up(&mut h, 1000.);
    println!("dragged far: {}", saved(&mut h));
    assert_eq!(saved(&mut h), 480.);

    // A double-click on the edge puts it back.
    down(&mut h, 477., 2);
    up(&mut h, 477.);
    println!("double-clicked: {}", saved(&mut h));
    assert_eq!(saved(&mut h), 260.);

    // On the right, its edge faces left: dragging left widens it.
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.interface.toolbar_layout = ToolbarLayout::Right, cx)
            })
            .unwrap()
    });
    h.pump();
    let edge = 1100. - 260. + 2.;
    down(&mut h, edge, 1);
    drag_to(&mut h, edge - 50.);
    up(&mut h, edge - 50.);
    println!("on the right, dragged left 50: {}", saved(&mut h));
    assert_eq!(saved(&mut h), 310.);
    drop(sessions);
}
