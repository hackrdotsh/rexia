//! Sidebar tabs against a real Rex server named by REXIA_TEST_SERVER: a tab's hover card
//! gives way to its right-click menu instead of covering it, and a tab row shows a close
//! button under the pointer that closes that tab without selecting it.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, point, px,
};
use rexia::{rex::host::HostModel, ui::workspace::hover_card::ShownCard};
use serde_json::Value;
use std::time::Duration;

fn move_to(h: &mut Harness, at: Point<Pixels>) {
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Modifiers::default(),
    }));
}

fn click(h: &mut Harness, at: Point<Pixels>, button: MouseButton) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
}

fn tabs(m: &HostModel) -> Vec<String> {
    m.view()
        .map(|v| v.windows.iter().map(|w| w.id().to_owned()).collect())
        .unwrap_or_default()
}

fn card(h: &mut Harness) -> bool {
    h.cx.update(|cx| {
        cx.try_global::<ShownCard>()
            .is_some_and(|c| c.target.is_some())
    })
}

#[test]
fn sidebar_tabs_close_on_hover_and_menus_beat_cards() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-side-tab-close");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.run("client.tab.new", Value::Null);
    let ids = h.wait("two tabs", |m| {
        let t = tabs(m);
        (t.len() == 2).then_some(t)
    });
    let (first, second) = (ids[0].clone(), ids[1].clone());
    let row = |h: &mut Harness, id: &str| {
        let key = format!("side-tab:{id}");
        h.wait_ui("the sidebar row", move |ws| ws.element_bounds(&key))
    };

    // Rest on the first tab's row: its card shows. Right-click it: the menu wins.
    let bounds = row(&mut h, &first);
    let middle = point(bounds.origin.x + px(60.), bounds.center().y);
    move_to(&mut h, point(px(600.), px(700.)));
    h.cx.advance_clock(Duration::from_millis(1000));
    h.pump();
    move_to(&mut h, middle);
    h.cx.advance_clock(Duration::from_millis(700));
    h.pump();
    let before = card(&mut h);
    click(&mut h, middle, MouseButton::Right);
    let menu = h.wait_ui("the tab menu", |ws| ws.context_menu_labels());
    h.cx.advance_clock(Duration::from_millis(100));
    h.pump();
    let after = card(&mut h);
    println!("card before right-click: {before}; menu {menu:?}; card with the menu open: {after}");
    assert!(before, "the card shows on rest");
    assert!(!after, "the menu isn't covered");
    h.screenshot("side-tab-menu");
    h.press("escape");

    // The close button: under the pointer at the row's end, it closes that tab only.
    let bounds = row(&mut h, &first);
    let close = point(
        bounds.origin.x + bounds.size.width - px(20.),
        bounds.center().y,
    );
    move_to(&mut h, close);
    h.screenshot("side-tab-close");
    click(&mut h, close, MouseButton::Left);
    let left = h.wait("the tab closed", |m| {
        let t = tabs(m);
        (t.len() == 1).then_some(t)
    });
    let selected = h.wait("the selected tab", |m| m.view()?.active_window_id.clone());
    println!("closed {first}; left {left:?}, selected {selected}");
    assert_eq!(left, std::slice::from_ref(&second));
    assert_eq!(selected, second);
    drop(sessions);
}
