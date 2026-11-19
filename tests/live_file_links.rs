//! Exercise printed links through the terminal and the confirmation dialog, not the opener.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point,
};
use std::{cell::RefCell, rc::Rc};

fn point(h: &mut Harness, x: u16, y: u16) -> Point<Pixels> {
    let window = h.window;
    h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(x, y).unwrap()
    })
}

fn click(h: &mut Harness, x: u16, y: u16, button: MouseButton, control: bool) {
    let at = point(h, x, y);
    let modifiers = Modifiers {
        control,
        ..Default::default()
    };
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers,
    }));
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        position: at,
        button,
        modifiers,
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        position: at,
        button,
        modifiers,
        click_count: 1,
    }));
}

#[test]
fn wrapped_relative_files_open_from_either_row_and_risky_files_ask() {
    let Some(address) = common::server() else {
        return;
    };
    let host = common::connect(&address);
    let sessions = Sessions::create(&host, "rexia-file-links");
    let mut h = Harness::open(&address, &sessions.ids[0], 850., 700.);
    h.wait("prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("exec /bin/sh -i");
    h.press("enter");

    let dir = tempfile::tempdir().unwrap();
    let name = format!("{} notes.txt", "a".repeat(120));
    let ordinary = dir.path().join(&name);
    let script = dir.path().join("local helper.sh");
    std::fs::write(&ordinary, "plain text\n").unwrap();
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let expected_url = rexia::quick_look::file_uri(&ordinary);
    let script_url = rexia::quick_look::file_uri(&script);
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()));
            })
            .unwrap();
    });

    h.type_text(&format!(
        "cd '{}'; clear; printf '%s\\n' 'FILE: \"{name}\":12:4,' 'SCRIPT: \"./local helper.sh\"'",
        dir.path().display(),
    ));
    h.press("enter");
    let (file, risky) = h.wait("printed relative file and script", |m| {
        let text = focused_text(m);
        Some((
            text.lines()
                .position(|line| line.starts_with("FILE: \"aaa"))? as u16,
            text.lines()
                .position(|line| line == "SCRIPT: \"./local helper.sh\"")? as u16,
        ))
    });
    assert!(risky >= file + 2, "the fixture must wrap the printed file");

    let continuation = point(&mut h, 2, file + 1);
    let modifiers = Modifiers {
        control: true,
        ..Default::default()
    };
    h.input(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers,
        capslock: Default::default(),
    }));
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: continuation,
        pressed_button: None,
        modifiers,
    }));
    h.screenshot("file-link-wrapped-hover");
    h.input(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers: Default::default(),
        capslock: Default::default(),
    }));

    click(&mut h, 9, file, MouseButton::Left, true);
    h.wait_ui("first row opened", |_| {
        (opened.borrow().len() == 1).then_some(())
    });
    click(&mut h, 2, file + 1, MouseButton::Left, true);
    h.wait_ui("continuation opened", |_| {
        (opened.borrow().len() == 2).then_some(())
    });
    assert_eq!(*opened.borrow(), [expected_url.clone(), expected_url]);

    click(&mut h, 18, risky, MouseButton::Right, false);
    let labels = h.wait_ui("script context menu", |ws| ws.context_menu_labels());
    assert!(labels.contains(&"Open File".to_owned()));
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.choose_context_item("Open File", window, cx)
            })
            .unwrap();
    });
    h.wait_ui("script confirmation", |ws| ws.confirm_title());
    assert_eq!(opened.borrow().len(), 2);
    h.screenshot("file-link-open-anyway");
    h.press("enter");
    assert_eq!(opened.borrow().len(), 2, "Enter defaults to Cancel");
    assert!(h.workspace(|ws| ws.confirm_title()).is_none());

    click(&mut h, 18, risky, MouseButton::Left, true);
    h.wait_ui("script confirmation again", |ws| ws.confirm_title());
    h.press("tab");
    h.press("enter");
    assert_eq!(opened.borrow().last(), Some(&script_url));
    println!(
        "Both wrapped rows opened {ordinary:?}; only Open Anyway sent {script:?} to the opener"
    );
}
