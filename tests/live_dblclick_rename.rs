//! Double-click to rename, against a real Rex server named by REXIA_TEST_SERVER: a
//! double-click on a tab that wasn't selected only selects it (as in Rex for Mac); one on
//! the selected tab, in the tab bar or the sidebar, renames it; one on a pane's title
//! renames the pane.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point};
use rexia::rex::host::HostModel;
use serde_json::Value;

fn double_click(h: &mut Harness, at: Point<Pixels>) {
    for clicks in [1, 2] {
        h.input(PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        }));
        h.input(PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: clicks,
        }));
    }
}

fn center(h: &mut Harness, key: &str) -> Point<Pixels> {
    let key = key.to_owned();
    h.wait_ui(&format!("{key} drawn"), move |ws| ws.element_bounds(&key))
        .center()
}

fn selected(m: &HostModel) -> Option<String> {
    m.view()?.active_window_id.clone()
}

#[test]
fn double_clicks_rename_selected_tabs_and_pane_titles() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-dblclick-rename");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let first = h.wait("the first tab", selected);
    h.run("client.tab.new", Value::Null);
    h.wait("the second tab", |m| selected(m).filter(|s| *s != first));

    // The first tab isn't selected: a double-click selects it, nothing more.
    let at = center(&mut h, &format!("tab:{first}"));
    double_click(&mut h, at);
    h.wait("the first tab selected", |m| {
        (selected(m).as_deref() == Some(first.as_str())).then_some(())
    });
    let renaming = h.workspace(|ws| ws.renaming());
    println!("double-click on an unselected tab: selected it, renaming {renaming:?}");
    assert_eq!(renaming, None);

    // Now it's selected: a double-click renames it.
    double_click(&mut h, at);
    let renaming = h.wait_ui("the rename prompt", |ws| ws.renaming());
    println!("double-click on the selected tab: renaming {renaming}");
    assert_eq!(renaming, format!("tab {first}"));
    h.press("escape");

    // The same from the sidebar's row for it.
    let at = center(&mut h, &format!("side-tab:{first}"));
    double_click(&mut h, at);
    let renaming = h.wait_ui("the rename prompt from the sidebar", |ws| ws.renaming());
    println!("double-click on its sidebar row: renaming {renaming}");
    assert_eq!(renaming, format!("tab {first}"));
    h.press("escape");

    // A pane's title: rename it, and the new name shows.
    let block = h.wait("the focused pane", |m| {
        m.view()?.active_window()?.focused_block_id.clone()
    });
    let at = center(&mut h, &format!("header:{block}"));
    double_click(&mut h, at);
    let renaming = h.wait_ui("the pane rename prompt", |ws| ws.renaming());
    println!("double-click on the pane title: renaming {renaming}");
    assert_eq!(renaming, format!("pane {block}"));
    h.type_text("build logs");
    h.press("enter");
    let label = h.wait("the pane's new name", |m| {
        let label = m.view()?.active_window()?.block(&block)?.label.clone();
        (label == "build logs").then_some(label)
    });
    println!("the pane is now called {label:?}");
    h.screenshot("dblclick-rename");
    drop(sessions);
}
