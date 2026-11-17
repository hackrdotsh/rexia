//! Session rows in the sidebar against a real Rex server named by REXIA_TEST_SERVER:
//! the right-click menu renames, moves and closes a session; left clicks toggle disclosure;
//! dragging a row reorders sessions on the server; and the row's close button closes it
//! after asking.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, point, px,
};
use rexia::rex::host::HostModel;

fn down(h: &mut Harness, at: Point<Pixels>, button: MouseButton, clicks: usize) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: clicks,
        first_mouse: false,
    }));
}

fn up(h: &mut Harness, at: Point<Pixels>, button: MouseButton) {
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
}

fn move_to(h: &mut Harness, at: Point<Pixels>, pressed: bool) {
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: pressed.then_some(MouseButton::Left),
        modifiers: Modifiers::default(),
    }));
}

/// Where our sessions sit in the server's order, as indexes into `ours`.
fn order(m: &HostModel, ours: &[String]) -> Vec<usize> {
    m.sessions
        .iter()
        .filter_map(|s| ours.iter().position(|o| *o == s.session_id))
        .collect()
}

fn row(h: &mut Harness, id: &str) -> Bounds<Pixels> {
    let key = format!("session:0:{id}");
    h.wait_ui("the session row", move |ws| ws.element_bounds(&key))
}

#[test]
fn session_rows_rename_move_reorder_and_close() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-rows-a");
    sessions.add("rexia-rows-b");
    sessions.add("rexia-rows-c");
    let ours = sessions.ids.clone();
    let mut h = Harness::open(&address, &ours[0], 1100., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let start = h.wait("our sessions", |m| {
        let o = order(m, &ours);
        (o.len() == 3).then_some(o)
    });
    println!("order at first: {start:?}");
    assert_eq!(start, [0, 1, 2]);

    // The menu: Move Down puts a after b.
    let a = row(&mut h, &ours[0]);
    down(&mut h, a.center(), MouseButton::Right, 1);
    up(&mut h, a.center(), MouseButton::Right);
    let labels = h.wait_ui("the session menu", |ws| ws.context_menu_labels());
    println!("session menu: {labels:?}");
    let window = h.window;
    h.screenshot("session-menu");
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.choose_context_item("Move Down", window, cx)
            })
            .unwrap()
    });
    let moved = h.wait("a after b", |m| {
        let o = order(m, &ours);
        (o == [1, 0, 2]).then_some(o)
    });
    println!("after Move Down: {moved:?}");

    // Drag c above the top row.
    let c = row(&mut h, &ours[2]);
    let top = row(&mut h, &ours[1]);
    down(&mut h, c.center(), MouseButton::Left, 1);
    move_to(&mut h, point(c.center().x, c.center().y - px(20.)), true);
    let target = point(top.center().x, top.top() + px(4.));
    move_to(&mut h, target, true);
    h.screenshot("session-drag");
    up(&mut h, target, MouseButton::Left);
    let dragged = h.wait("c first", |m| {
        let o = order(m, &ours);
        (o == [2, 1, 0]).then_some(o)
    });
    println!("after dragging c to the top: {dragged:?}");

    // Double-clicking doesn't rename; the right-click menu does.
    let b = row(&mut h, &ours[1]);
    down(&mut h, b.center(), MouseButton::Left, 1);
    up(&mut h, b.center(), MouseButton::Left);
    down(&mut h, b.center(), MouseButton::Left, 2);
    up(&mut h, b.center(), MouseButton::Left);
    assert!(h.workspace(|ws| ws.renaming().is_none()));
    down(&mut h, b.center(), MouseButton::Right, 1);
    up(&mut h, b.center(), MouseButton::Right);
    h.wait_ui("the session menu", |ws| ws.context_menu_labels());
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.choose_context_item("Rename Session…", window, cx)
            })
            .unwrap()
    });
    let renaming = h.wait_ui("the rename dialog", |ws| ws.renaming());
    assert_eq!(renaming, format!("session {}", ours[1]));
    h.press("escape");

    // The close button asks, then closes.
    let c = row(&mut h, &ours[2]);
    let close = point(c.right() - px(20.), c.center().y);
    move_to(&mut h, close, false);
    down(&mut h, close, MouseButton::Left, 1);
    up(&mut h, close, MouseButton::Left);
    let title = h.wait_ui("the close question", |ws| ws.confirm_title());
    println!("close button asks {title:?}");
    h.press("tab");
    h.press("enter");
    let left = h.wait("c closed", |m| {
        let o = order(m, &ours);
        (o.len() == 2).then_some(o)
    });
    println!("after closing c: {left:?}");
    assert!(!left.contains(&2));
    drop(sessions);
}
