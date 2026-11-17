//! Quick Look collects visible text/OSC 8 links and consumes navigation keys.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput};
use rexia::quick_look::Target;
use std::{cell::RefCell, rc::Rc};

#[test]
fn quick_look_steps_through_visible_links() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start_with(true) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("one.txt");
    let second = directory.path().join("two.txt");
    std::fs::write(&first, "first preview").unwrap();
    std::fs::write(&second, "second preview").unwrap();
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-preview-navigation");
    let mut h = Harness::open(&address, &sessions.ids[0], 1300., 800.);
    h.wait("prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let record = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_preview_bus(Some(desktop.address.clone()));
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()));
            })
            .unwrap()
    });
    h.type_text(&format!(
        "printf '%b\\n' '\\033[2J\\033[H{}\\n{}\\nhttps://example.com/plain\\n\\033]8;;https://example.com/osc\\033\\\\Named-link\\033]8;;\\033\\\\'",
        first.display(), second.display()
    ));
    h.press("enter");
    h.wait("all links displayed", |m| {
        let text = focused_text(m);
        (text.lines().next() == Some(first.to_str().unwrap()) && text.contains("Named-link"))
            .then_some(())
    });

    // No pointer target: keyboard Quick Look starts at the first visible link.
    h.run("client.quick_look", serde_json::Value::Null);
    h.wait_ui("four preview targets", |ws| {
        (ws.quick_look_selection()?.1 == 4).then_some(())
    });
    let selection = h.workspace(|ws| ws.quick_look_selection().unwrap());
    assert_eq!(selection, (0, 4, Target::Path(first.display().to_string())));
    h.wait_ui("first file preview", |_| {
        (desktop.previewed().len() == 1).then_some(())
    });
    h.press("right");
    h.wait_ui("second file preview", |_| {
        (desktop.previewed().len() == 2).then_some(())
    });
    assert_eq!(
        h.workspace(|ws| ws.quick_look_selection().unwrap().2),
        Target::Path(second.display().to_string())
    );
    h.press("right");
    assert_eq!(
        opened.borrow().last().map(String::as_str),
        Some("https://example.com/plain")
    );
    h.press("right");
    assert_eq!(
        opened.borrow().last().map(String::as_str),
        Some("https://example.com/osc")
    );
    println!("arrow keys visited two files, a text URL, and an OSC 8 link");
    h.screenshot("quick-look-navigation");
    h.press("escape");
    assert!(h.workspace(|ws| ws.quick_look_selection()).is_none());

    // Point at the second file: it becomes the first preview shown, not the first row.
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(4, 1).unwrap()
    });
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Default::default(),
    }));
    h.run("client.quick_look", serde_json::Value::Null);
    h.wait_ui("pointer target selected", |ws| {
        let choice = ws.quick_look_selection()?;
        (choice.0 == 1 && choice.1 == 4).then_some(())
    });
    h.press("escape");
    let modifiers = Modifiers {
        control: true,
        shift: true,
        ..Default::default()
    };
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
    }));
    h.wait_ui("single-target preview", |ws| {
        (ws.quick_look_selection()?.1 == 1).then_some(())
    });
    println!("pointer selects the second file; single-link Quick Look keeps only that file");
    drop(sessions);
}
