//! Quick Look against a real Rex server named by REXIA_TEST_SERVER: files under the pointer
//! go to GNOME's previewer (a stand-in on a private bus), or to the desktop's opener
//! without one; web links go to the opener. A recorder stands in for the opener.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use gpui::{MouseMoveEvent, PlatformInput};
use rexia::ui::workspace::quick_look::Shown;
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

/// Point at the cell at `col`, `row` of the focused pane.
fn point(h: &mut Harness, col: u16, row: u16) {
    let window = h.window;
    h.pump();
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(col, row).unwrap()
    });
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Default::default(),
    }));
}

/// Quick Look, and what it showed (or the notice it gave).
fn quick_look(h: &mut Harness) -> Result<Shown, String> {
    let before = h.workspace(|ws| ws.quick_looks().len());
    let window = h.window;
    h.cx.update(|cx| window.update(cx, |ws, _, _| ws.notice = None).unwrap());
    h.run("client.quick_look", Value::Null);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.pump();
        let (shown, notice) =
            h.workspace(|ws| (ws.quick_looks().get(before).cloned(), ws.notice.clone()));
        if let Some(shown) = shown {
            return Ok(shown);
        }
        if let Some(notice) = notice {
            return Err(notice);
        }
        assert!(Instant::now() < deadline, "Quick Look did nothing");
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn quick_look_previews_files_and_opens_links() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start_with(true) else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-quick-look");
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

    // A file and a web link on screen.
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
    });
    let link_col = (notes.display().to_string().len() + 3) as u16;

    // With nothing under the pointer, it says so.
    point(&mut h, link_col + 30, line as u16);
    let nothing = quick_look(&mut h);
    println!("pointing at nothing: {nothing:?}");
    assert!(nothing.is_err());

    // The file goes to the previewer.
    point(&mut h, 3, line as u16);
    let file = quick_look(&mut h).unwrap();
    println!(
        "pointing at the file: {file:?}; the previewer got {:?}",
        desktop.previewed()
    );
    assert!(matches!(&file, Shown::Previewer(uri) if uri.ends_with("/notes.md")));
    assert_eq!(desktop.previewed().len(), 1);

    // The web link goes to the opener.
    point(&mut h, link_col, line as u16);
    let web = quick_look(&mut h).unwrap();
    println!("pointing at the link: {web:?}");
    assert_eq!(web, Shown::Opener("https://rex.dev/docs".into()));

    // Without the previewer, the file goes to the opener too.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_preview_bus(Some("unix:path=/nonexistent".into()))
            })
            .unwrap()
    });
    point(&mut h, 3, line as u16);
    let fallback = quick_look(&mut h).unwrap();
    println!(
        "without the previewer: {fallback:?}; opened {:?}",
        opened.borrow()
    );
    assert!(matches!(&fallback, Shown::Opener(uri) if uri.ends_with("/notes.md")));
    assert_eq!(opened.borrow().len(), 2);
    drop(sessions);
}
