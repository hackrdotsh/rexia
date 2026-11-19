//! Terminal › Selection and Links against a real Rex server named by REXIA_TEST_SERVER:
//! selection colors and the copy shine, then ctrl+clicking links a program prints, with a
//! recorder standing in for the desktop's link handler. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
use rexia::{
    settings::{AppLinks, SelectionColor, Settings},
    ui::theme::{Theme, color},
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn pane<R>(h: &mut Harness, read: impl FnOnce(&rexia::ui::terminal::TerminalPane) -> R) -> R {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        let ws = window.read(cx).unwrap();
        let pane = ws.focused_terminal().expect("a focused pane");
        read(pane.read(cx))
    })
}

/// Ctrl+click the cell at `col`, `row` of the focused pane.
fn ctrl_click(h: &mut Harness, col: u16, row: u16) {
    ctrl_click_with_activation(h, col, row, false);
}

fn ctrl_click_with_activation(h: &mut Harness, col: u16, row: u16, first_mouse: bool) {
    let at = pane(h, |p| p.cell_point(col, row)).expect("a measured pane");
    let modifiers = Modifiers {
        control: true,
        ..Default::default()
    };
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
        first_mouse,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
    }));
    for _ in 0..20 {
        h.pump();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn selection_looks_and_link_prompts() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-selection-links");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let theme = h.cx.update(|cx| Theme::get(cx).clone());

    // Two links a program prints: an ssh:// link on the first row, a web link below.
    h.type_text(r"clear; printf '\033]8;;ssh://me@build\033\\SSHLINK\033]8;;\033\\\n\033]8;;https://example.com/rexia\033\\WEBLINK\033]8;;\033\\\n'");
    h.press("enter");
    h.wait("the links", |m| {
        let text = focused_text(m);
        (text.starts_with("SSHLINK") && text.contains("WEBLINK")).then_some(())
    });

    // Selection: the program's text colors kept, terminal color 4 behind, rounded.
    settings(&mut h, |s| {
        s.terminal.selection.foreground = SelectionColor::Keep;
        s.terminal.selection.background = SelectionColor::Palette(4);
        s.terminal.selection.rounded = true;
    });
    h.run("pane.select_all", Value::Null);
    let colors = pane(&mut h, |p| p.selection_colors());
    println!("selection colors: {colors:?}");
    assert_eq!(colors, Some((None, color(theme.terminal.ansi[4]))));
    // Copying shines across the selection; with the animation off it doesn't.
    h.run("pane.copy", Value::Null);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while pane(&mut h, |p| p.copy_shines()) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "no shine after copying"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.screenshot("selection-rounded");
    settings(&mut h, |s| s.terminal.selection.copy_animation = false);
    h.run("pane.copy", Value::Null);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let shines = pane(&mut h, |p| p.copy_shines());
    println!("shines after two copies, one with the animation off: {shines}");
    assert_eq!(shines, 1);
    h.press("escape");

    // Links: a recorder stands in for the desktop's handler.
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let record = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()))
            })
            .unwrap()
    });

    // The click that activated the window focuses the terminal but opens nothing.
    ctrl_click_with_activation(&mut h, 2, 1, true);
    assert!(opened.borrow().is_empty());
    // The next click opens the web link.
    ctrl_click(&mut h, 2, 1);
    println!("after the web link: {:?}", opened.borrow());
    assert_eq!(*opened.borrow(), ["https://example.com/rexia"]);

    // An ssh:// link asks first; Always Open opens it and remembers ssh://build.
    ctrl_click(&mut h, 2, 0);
    let asked = h.workspace(|ws| ws.asked_link().map(str::to_owned));
    println!("asked about: {asked:?}");
    assert_eq!(asked.as_deref(), Some("ssh://me@build"));
    h.screenshot("link-prompt");
    h.press("left");
    h.press("enter");
    let trusted = h.workspace(|ws| ws.settings.terminal.trusted_links.clone());
    println!("trusted: {trusted:?}, opened: {:?}", opened.borrow());
    assert_eq!(trusted, ["ssh://build"]);
    assert_eq!(
        opened.borrow().last().map(String::as_str),
        Some("ssh://me@build")
    );

    // Trusted now: it opens without asking.
    ctrl_click(&mut h, 2, 0);
    assert_eq!(h.workspace(|ws| ws.asked_link().map(str::to_owned)), None);
    assert_eq!(opened.borrow().len(), 3);

    // Never: app links don't open, trusted or not, and a notice says why.
    settings(&mut h, |s| s.terminal.app_links = AppLinks::Never);
    ctrl_click(&mut h, 2, 0);
    let notice = h.workspace(|ws| ws.notice.clone());
    println!(
        "with Never: opened {}, notice {notice:?}",
        opened.borrow().len()
    );
    assert_eq!(opened.borrow().len(), 3);
    assert!(notice.is_some());
    drop(sessions);
}
