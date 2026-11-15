//! Dragging tabs and panes, against a real Rex server named by REXIA_TEST_SERVER: a tab
//! dropped before another moves there; Escape cancels a drag; a pane dragged by its header
//! onto another tab moves into it; a pane Ctrl-dragged from inside onto the bottom of another
//! goes below it; and a pane dropped after the last tab becomes a tab of its own.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, point, px,
};
use rexia::{
    rex::host::HostModel,
    ui::workspace::dragging::{Grab, Landing, Side},
};
use serde_json::{Value, json};

fn tabs(m: &HostModel) -> Vec<String> {
    m.view()
        .map(|v| v.windows.iter().map(|w| w.id().to_owned()).collect())
        .unwrap_or_default()
}

fn blocks_of(m: &HostModel, tab: &str) -> Vec<String> {
    m.view()
        .and_then(|v| v.window(tab))
        .map(|w| w.blocks().filter_map(|b| b.block_id.clone()).collect())
        .unwrap_or_default()
}

fn active_tab(m: &HostModel) -> Option<String> {
    m.view()?.active_window_id.clone()
}

fn ctrl() -> Modifiers {
    Modifiers {
        control: true,
        ..Default::default()
    }
}

/// Press at `from`, move through `to` with the button held, and stop there (not released).
fn drag_to(h: &mut Harness, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: from,
        modifiers,
        click_count: 1,
        first_mouse: false,
    }));
    for step in 1..=4 {
        let t = step as f32 / 4.;
        h.input(PlatformInput::MouseMove(MouseMoveEvent {
            position: point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }));
    }
}

fn release(h: &mut Harness, at: Point<Pixels>) {
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
}

fn bounds(h: &mut Harness, key: String) -> gpui::Bounds<Pixels> {
    h.wait_ui(&format!("{key} drawn"), move |ws| ws.element_bounds(&key))
}

fn landing(h: &mut Harness) -> Option<Landing> {
    h.workspace(|ws| ws.drag_landing())
}

#[test]
fn tabs_and_panes_drag_into_place() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-drag");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Three tabs; the first has two panes.
    let first_pane = h.wait("the first pane", |m| {
        m.view()?.active_window()?.focused_block_id.clone()
    });
    h.run("pane.split.right", Value::Null);
    let second_pane = h.wait("the second pane", |m| {
        m.view()?
            .active_window()?
            .focused_block_id
            .clone()
            .filter(|b| *b != first_pane)
    });
    h.run("client.tab.new", Value::Null);
    h.wait("two tabs", |m| (tabs(m).len() == 2).then_some(()));
    h.run("client.tab.new", Value::Null);
    let order = h.wait("three tabs", |m| {
        let t = tabs(m);
        (t.len() == 3).then_some(t)
    });
    let (t1, t2, t3) = (order[0].clone(), order[1].clone(), order[2].clone());
    h.run("client.tab.goto", json!({"index": 1}));
    h.wait("the first tab", |m| {
        (active_tab(m).as_deref() == Some(t1.as_str())).then_some(())
    });

    // The third tab, dropped on the first one's left half, moves before it.
    let from = bounds(&mut h, format!("tab:{t3}")).center();
    let first = bounds(&mut h, format!("tab:{t1}"));
    let to = point(first.left() + px(8.), first.center().y);
    drag_to(&mut h, from, to, Modifiers::default());
    assert_eq!(h.workspace(|ws| ws.dragging()), Some(Grab::Tab(t3.clone())));
    assert_eq!(landing(&mut h), Some(Landing::TabBefore(Some(t1.clone()))));
    h.screenshot("drag-tab");
    release(&mut h, to);
    let moved = h.wait("the reordered tabs", |m| {
        let t = tabs(m);
        (t[0] == t3).then_some(t)
    });
    println!("tabs after dragging the third first: {moved:?}");
    assert_eq!(moved, [t3.clone(), t1.clone(), t2.clone()]);

    // Escape drops a drag without moving anything.
    let from = bounds(&mut h, format!("tab:{t2}")).center();
    let to = bounds(&mut h, format!("tab:{t3}")).origin + point(px(4.), px(10.));
    drag_to(&mut h, from, to, Modifiers::default());
    assert!(landing(&mut h).is_some());
    h.press("escape");
    assert_eq!(h.workspace(|ws| ws.dragging()), None);
    release(&mut h, to);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(h.wait("tabs", |m| Some(tabs(m))), moved, "Escape cancelled");
    println!("Escape cancelled the drag");

    // The second pane, dragged by its header onto the last tab, moves into it.
    let header = bounds(&mut h, format!("header:{second_pane}"));
    let target = bounds(&mut h, format!("tab:{t2}")).center();
    drag_to(&mut h, header.center(), target, Modifiers::default());
    assert_eq!(landing(&mut h), Some(Landing::IntoTab(t2.clone())));
    h.screenshot("drag-pane-to-tab");
    release(&mut h, target);
    let joined = h.wait("the pane in the other tab", |m| {
        let b = blocks_of(m, &t2);
        (b.contains(&second_pane) && blocks_of(m, &t1) == [first_pane.clone()]).then_some(b)
    });
    println!("the last tab now holds {joined:?}");

    // Ctrl-dragged from inside onto the bottom of the tab's other pane, it goes below it.
    h.run("client.tab.goto", json!({"index": 3}));
    h.wait("the last tab", |m| {
        (active_tab(m).as_deref() == Some(t2.as_str())).then_some(())
    });
    let other = joined.iter().find(|b| **b != second_pane).cloned().unwrap();
    let mover = second_pane.clone();
    let from = h
        .wait_ui("the pane drawn", move |ws| ws.pane_bounds(&mover))
        .center();
    let anchor = {
        let other = other.clone();
        h.wait_ui("the other pane drawn", move |ws| ws.pane_bounds(&other))
    };
    let to = point(anchor.center().x, anchor.bottom() - px(20.));
    drag_to(&mut h, from, to, ctrl());
    assert_eq!(
        landing(&mut h),
        Some(Landing::Beside {
            anchor: other.clone(),
            side: Side::Bottom
        })
    );
    h.screenshot("drag-pane-beside");
    release(&mut h, to);
    let (below, above) = h.wait("the pane below the other", |m| {
        let panes = m.view()?.active_window()?.panes();
        let moved = panes.iter().find(|p| p.block_id == second_pane)?;
        let anchor = panes.iter().find(|p| p.block_id == other)?;
        (moved.rect.y > anchor.rect.y + 0.1).then_some((moved.rect, anchor.rect))
    });
    println!("below: {below:?}, above: {above:?}");
    assert!(
        (below.x - above.x).abs() < 0.05,
        "stacked, not side by side"
    );

    // Dropped after the last tab, it becomes a tab of its own at the end.
    let header = bounds(&mut h, format!("header:{second_pane}"));
    let last = bounds(&mut h, format!("tab:{t2}"));
    let to = point(last.right() + px(30.), last.center().y);
    drag_to(&mut h, header.center(), to, Modifiers::default());
    assert_eq!(landing(&mut h), Some(Landing::NewTab(None)));
    release(&mut h, to);
    let (all, own) = h.wait("a tab of its own", |m| {
        let t = tabs(m);
        let last = t.last()?.clone();
        (t.len() == 4 && blocks_of(m, &last) == [second_pane.clone()]).then(|| (t.clone(), last))
    });
    println!("tabs: {all:?}; the pane's own tab is {own}");
    drop(sessions);
}
