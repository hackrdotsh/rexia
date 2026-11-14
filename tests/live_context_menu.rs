//! The terminal's context menu against a real Rex server named by REXIA_TEST_SERVER:
//! right-click shows editing items, then what's under the pointer (a file or a web link),
//! then the pane's actions, and each item does what it says. The file manager is a
//! stand-in on a private bus; a recorder stands in for the link opener.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

/// Right-click the focused pane's cell at `col`, `row`, with Shift if `shift`.
fn right_click(h: &mut Harness, col: u16, row: u16, shift: bool) {
    let window = h.window;
    h.pump();
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(col, row).unwrap()
    });
    let modifiers = Modifiers {
        shift,
        ..Default::default()
    };
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Right,
        position: at,
        modifiers,
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Right,
        position: at,
        modifiers,
        click_count: 1,
    }));
}

fn menu(h: &mut Harness, what: &str) -> Vec<String> {
    h.wait_ui(what, |ws| ws.context_menu_labels())
}

fn choose(h: &mut Harness, label: &str) {
    let window = h.window;
    let label = label.to_owned();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.choose_context_item(&label, window, cx)
            })
            .unwrap()
    });
    h.pump();
}

#[test]
fn context_menu_offers_what_is_under_the_pointer() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start_with(true) else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-context-menu");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let record = opened.clone();
    let window = h.window;
    let bus = desktop.address.clone();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()));
                ws.set_preview_bus(Some(bus));
            })
            .unwrap()
    });

    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, "# notes\n").unwrap();
    h.type_text(&format!(
        "clear; printf '%s %s\\n' {} https://rex.dev/docs",
        notes.display()
    ));
    h.press("enter");
    let line = h.wait("the output", |m| {
        focused_text(m)
            .lines()
            .position(|l| l.starts_with(notes.to_str().unwrap()))
    }) as u16;
    let link_col = (notes.display().to_string().len() + 3) as u16;

    // On a file: editing (Copy is off without a selection), the file's items, the pane's.
    right_click(&mut h, 3, line, false);
    let labels = menu(&mut h, "the menu on a file");
    println!("on a file: {labels:?}");
    assert_eq!(&labels[..3], ["Copy (off)", "Paste", "Select All"]);
    for want in [
        "Open File",
        "Copy Path",
        "Show in Files",
        "Quick Look",
        "Split Right",
        "Clear Terminal",
        "Close Pane",
    ] {
        assert!(labels.iter().any(|l| l == want), "missing {want}");
    }
    h.screenshot("context-menu-file");
    choose(&mut h, "Show in Files");
    let shown = h.wait("the file manager", |_| {
        let shown = desktop.shown_in_files();
        (!shown.is_empty()).then_some(shown)
    });
    println!("the file manager was asked to show {shown:?}");
    assert!(shown[0].ends_with("/notes.md"));
    assert!(
        h.workspace(|ws| ws.context_menu_labels().is_none()),
        "choosing closes it"
    );

    // On a web link: open it.
    right_click(&mut h, link_col, line, false);
    let labels = menu(&mut h, "the menu on a link");
    println!("on a link: {labels:?}");
    assert!(labels.contains(&"Copy Link".to_owned()));
    choose(&mut h, "Open Link");
    println!("opened {:?}", opened.borrow());
    assert_eq!(
        opened.borrow().last().map(String::as_str),
        Some("https://rex.dev/docs")
    );

    // Escape closes it without doing anything.
    right_click(&mut h, link_col + 40, line, false);
    let labels = menu(&mut h, "the menu on nothing");
    assert!(!labels.iter().any(|l| l == "Open Link" || l == "Copy Path"));
    h.press("escape");
    assert!(h.workspace(|ws| ws.context_menu_labels().is_none()));

    // With a selection, Copy is on; arrows and Enter choose.
    h.run("pane.select_all", Value::Null);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        h.pump();
        let selected = h.cx.update(|cx| {
            let pane = window.read(cx).unwrap().focused_terminal()?;
            let frame = pane.read(cx).handle()?.snapshot().frame?;
            Some(frame.has_selection)
        });
        if selected == Some(true) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no selection");
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    right_click(&mut h, 0, 0, false);
    let labels = menu(&mut h, "the menu with a selection");
    println!("with a selection: {:?}", &labels[..3]);
    assert_eq!(labels[0], "Copy");

    // Clear Terminal is the server's block action.
    choose(&mut h, "Clear Terminal");
    h.wait("the screen cleared", |m| {
        (!focused_text(m).contains("rex.dev/docs")).then_some(())
    });
    println!("Clear Terminal cleared the screen");

    // Split Right from the keyboard: Down until it's highlighted, then Enter.
    let panes = |h: &mut Harness| {
        h.workspace(|ws| {
            ws.model()
                .view()
                .and_then(|v| v.active_window())
                .map_or(0, |w| w.panes().len())
        })
    };
    let before = panes(&mut h);
    right_click(&mut h, 0, 0, false);
    let labels = menu(&mut h, "the menu again");
    let enabled: Vec<&String> = labels.iter().filter(|l| !l.ends_with("(off)")).collect();
    let steps = enabled.iter().position(|l| *l == "Split Right").unwrap() + 1;
    for _ in 0..steps {
        h.press("down");
    }
    h.press("enter");
    let after = h.wait("the split", |m| {
        let n = m.view()?.active_window()?.panes().len();
        (n > before).then_some(n)
    });
    println!("Split Right from the keyboard: {before} -> {after} panes");
    drop(sessions);
}
