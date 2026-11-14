//! Pasting and dropping on a terminal on this machine, against a real Rex server named by
//! REXIA_TEST_SERVER: a paste that would run as it's pasted asks first, copied files paste as
//! their quoted paths, an image pastes as a file only this user can read, and dropped files
//! type their paths.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{ClipboardItem, ExternalPaths, FileDropEvent, Image, ImageFormat, PlatformInput};
use std::os::unix::fs::PermissionsExt;

fn paste(h: &mut Harness, item: ClipboardItem) {
    let window = h.window;
    h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.update(cx, |pane, cx| pane.paste_item(item, cx));
    });
    h.pump();
}

fn asking(h: &mut Harness) -> Option<String> {
    h.workspace(|ws| ws.confirm_title())
}

/// Clear the prompt's line for the next check.
fn interrupt(h: &mut Harness) {
    h.press("ctrl-c");
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
}

#[test]
fn pastes_ask_when_unsafe_and_files_paste_as_paths() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-paste-drop");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // cat hasn't asked for bracketed paste, so line breaks would run: rexia asks.
    h.type_text("cat");
    h.press("enter");
    std::thread::sleep(std::time::Duration::from_millis(400));
    paste(
        &mut h,
        ClipboardItem::new_string("first-line\nsecond-line\n".into()),
    );
    let title = h.wait_ui("the paste question", |ws| ws.confirm_title());
    println!("unbracketed multi-line paste asks: {title:?}");
    h.screenshot("paste-confirm");
    // Enter starts on Cancel; Tab moves to Paste.
    h.press("tab");
    h.press("enter");
    h.wait("the paste after Paste", |m| {
        (focused_text(m).matches("second-line").count() >= 2).then_some(())
    });
    println!("confirmed: cat echoed both lines");
    interrupt(&mut h);

    // A program that asks for bracketed paste gets it without a question.
    h.type_text("printf '\\033[?2004h'; cat");
    h.press("enter");
    std::thread::sleep(std::time::Duration::from_millis(400));
    paste(
        &mut h,
        ClipboardItem::new_string("bracketed-one\nbracketed-two\n".into()),
    );
    h.wait("the bracketed paste", |m| {
        focused_text(m).contains("bracketed-two").then_some(())
    });
    assert_eq!(asking(&mut h), None, "no question with bracketed paste");
    println!("bracketed paste went straight through");
    interrupt(&mut h);
    h.type_text("printf '\\033[?2004l'");
    h.press("enter");
    interrupt(&mut h);

    // Copied files paste as their paths, quoted for the shell.
    let dir = tempfile::tempdir().unwrap();
    let spaced = dir.path().join("a b.md");
    std::fs::write(&spaced, "notes").unwrap();
    paste(
        &mut h,
        ClipboardItem::new_string(format!(
            "file://{}\n",
            spaced.display().to_string().replace(' ', "%20")
        )),
    );
    let quoted = format!("'{}'", spaced.display());
    h.wait("the copied file's path", |m| {
        focused_text(m)
            .replace('\n', "")
            .contains(&quoted)
            .then_some(())
    });
    println!("copied file pasted as {quoted}");
    interrupt(&mut h);

    // An image pastes as a file only this user can read.
    let png = b"\x89PNG\r\n\x1a\n rexia test image".to_vec();
    paste(
        &mut h,
        ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, png.clone())),
    );
    let path = h.wait("the image's path", |m| {
        // A long path wraps; join the screen's lines to read it.
        let text = focused_text(m).replace('\n', "");
        let end = text.find("Pasted image.png'")?;
        let start = text[..end].rfind('\'')? + 1;
        Some(format!("{}Pasted image.png", &text[start..end]))
    });
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    println!("image pasted as {path} (mode {mode:o})");
    assert_eq!(std::fs::read(&path).unwrap(), png);
    assert_eq!(mode, 0o400);
    interrupt(&mut h);

    // Dropped files type their paths.
    let other = dir.path().join("dropped.txt");
    std::fs::write(&other, "x").unwrap();
    let window = h.window;
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(4, 4).unwrap()
    });
    let paths = ExternalPaths(vec![other.clone(), spaced.clone()].into());
    h.input(PlatformInput::FileDrop(FileDropEvent::Entered {
        position: at,
        paths,
    }));
    // The drop comes straight after typing, as when files are dragged in from another
    // app while the keyboard was last used.
    h.input(PlatformInput::FileDrop(FileDropEvent::Pending {
        position: at,
    }));
    h.input(PlatformInput::FileDrop(FileDropEvent::Submit {
        position: at,
    }));
    let typed = format!("{} {quoted}", other.display());
    h.wait("the dropped paths", |m| {
        focused_text(m)
            .replace('\n', "")
            .contains(&typed)
            .then_some(())
    });
    println!("dropped files typed: {typed}");
    interrupt(&mut h);
    drop(sessions);
}
