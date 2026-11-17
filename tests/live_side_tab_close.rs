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
use std::{sync::Mutex, time::Duration};

// Both tests share a server; creating or deleting sessions moves sidebar click targets.
static SIDEBAR_FIXTURE: Mutex<()> = Mutex::new(());

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
fn sidebar_spaces_expand_without_switching_and_stay_open_on_focus_changes() {
    let Some(address) = common::server() else {
        return;
    };
    let _fixture = SIDEBAR_FIXTURE
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-space-first");
    let first = sessions.ids[0].clone();
    let second = sessions.add("rexia-space-second");
    let mut h = Harness::open(&address, &first, 1200., 760.);
    let first_tab = h.wait("first space tab", |m| m.view()?.active_window_id.clone());
    let second_disclosure = format!("session:0:{second}");
    let toggle = |h: &mut Harness, key: &str| {
        let bounds = h.wait_ui("space disclosure", |ws| ws.element_bounds(key));
        click(h, bounds.center(), MouseButton::Left);
    };
    toggle(&mut h, &second_disclosure);
    let second_tab = h.wait("background space tabs loaded", |m| {
        m.views.get(&second)?.active_window_id.clone()
    });
    let row = |id: &str| format!("side-tab:{id}");
    let add = h.wait_ui("add tab button in the second space", |ws| {
        ws.element_bounds(&format!("session-new-tab:0:{second}"))
    });
    move_to(&mut h, add.center());
    click(&mut h, add.center(), MouseButton::Left);
    let added = h.wait("a new tab in the background space", |m| {
        m.views
            .get(&second)?
            .windows
            .iter()
            .find(|w| w.id() != second_tab)
            .map(|w| w.id().to_owned())
    });
    h.wait_ui("new background tab row", |ws| {
        ws.element_bounds(&row(&added))
    });
    assert_eq!(h.host.snapshot().selected.as_deref(), Some(first.as_str()));
    assert_eq!(tabs(&h.host.snapshot()), std::slice::from_ref(&first_tab));
    h.screenshot("sidebar-background-tabs");
    let second_bounds = h.wait_ui("background tab row", |ws| {
        ws.element_bounds(&row(&second_tab))
    });
    assert_eq!(h.host.snapshot().selected.as_deref(), Some(first.as_str()));
    click(&mut h, second_bounds.center(), MouseButton::Left);
    h.wait("second space selected", |m| {
        (m.selected.as_ref() == Some(&second)).then_some(())
    });
    assert!(h.workspace(|ws| ws.element_bounds(&row(&first_tab)).is_some()));

    toggle(&mut h, &format!("session:0:{first}"));
    h.wait_ui("manually closed first space", |ws| {
        ws.element_bounds(&row(&first_tab)).is_none().then_some(())
    });
    assert!(h.workspace(|ws| ws.element_bounds(&row(&second_tab)).is_some()));

    toggle(&mut h, "host-disclosure:test");
    h.wait_ui("collapsed host hides sessions and tabs", |ws| {
        (ws.element_bounds(&format!("session:0:{first}")).is_none()
            && ws.element_bounds(&row(&second_tab)).is_none())
        .then_some(())
    });
    assert_eq!(h.host.snapshot().selected.as_deref(), Some(second.as_str()));
    toggle(&mut h, "host-disclosure:test");
    h.wait_ui("expanded host restores the open session", |ws| {
        ws.element_bounds(&row(&second_tab))
    });
    assert!(h.workspace(|ws| ws.element_bounds(&row(&first_tab)).is_none()));

    toggle(&mut h, &format!("session:0:{first}"));
    let first_bounds = h.wait_ui("session row restores its tabs", |ws| {
        ws.element_bounds(&row(&first_tab))
    });
    assert_eq!(h.host.snapshot().selected.as_deref(), Some(second.as_str()));
    click(&mut h, first_bounds.center(), MouseButton::Left);
    h.wait("first space selected again", |m| {
        (m.selected.as_ref() == Some(&first)).then_some(())
    });
    h.run("session.collapse_others", Value::Null);
    h.wait_ui("Collapse Other Sessions hides background tabs", |ws| {
        ws.element_bounds(&row(&second_tab)).is_none().then_some(())
    });
    assert!(h.workspace(|ws| ws.element_bounds(&row(&first_tab)).is_some()));
}

#[test]
fn sidebar_tabs_close_on_hover_and_menus_beat_cards() {
    let Some(address) = common::server() else {
        return;
    };
    let _fixture = SIDEBAR_FIXTURE
        .lock()
        .unwrap_or_else(|err| err.into_inner());
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
