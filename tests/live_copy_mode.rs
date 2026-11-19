//! Run against a disposable Rex server via REXIA_TEST_SERVER. Load
//! fixtures/copy_mode.lua and set REXIA_TEST_COPY_MODE_LUA=1 to check overrides.
mod common;

use common::{Harness, Sessions, focused_text, request};
use gpui::{ClipboardItem, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
use rexia::rex::{data::SHELL_FLAVOR, host::HostModel, stream::Frame};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn frame(model: &HostModel) -> Option<Arc<Frame>> {
    let block = model.view()?.active_window()?.focused_block_id.as_ref()?;
    model.panes.get(block)?.snapshot().frame
}

fn wait_clipboard(h: &mut Harness, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.pump();
        let actual =
            h.cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
        if actual
            .as_deref()
            .is_some_and(|text| text.trim_end() == expected)
        {
            println!("Clipboard: {actual:?}");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {expected:?}, clipboard was {actual:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn enter_move_and_yank_with_the_keyboard_against_rex() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-copy-mode");
    let session = &sessions.ids[0];
    // Use a known shell and leave it open for the input-isolation check.
    request(
        &control,
        "session.new_window",
        json!({
            "session_id": session,
            "layout": {"block": {"flavor": SHELL_FLAVOR, "options": {
                "shell": "none", "command": ["bash", "--noprofile", "--norc", "-c",
                    "printf 'history-%03d\\n' {1..100}; printf '\\ncopy-live-tail\\n'; PS1='copy> ' exec bash --noprofile --norc -i"]
            }}}
        }),
    )
    .unwrap();
    let mut h = Harness::open(&address, session, 1000., 660.);
    h.activate();
    h.wait("a terminal with real scrollback and a keymap", |model| {
        let frame = frame(model)?;
        (model.keymap.is_some()
            && frame.scrollbar.offset > 20
            && focused_text(model).contains("\ncopy-live-tail\n"))
        .then_some(())
    });

    let rows = h.wait_ui("copy bindings in Shortcuts", |ws| {
        let rows = ws.shortcut_rows("copy mode");
        rows.iter()
            .any(|r| r.keys.iter().any(|key| key == "copy: V"))
            .then_some(rows)
    });
    assert!(rows.iter().any(|r| r.title == "Enter Copy Mode"
        && r.editable
        && r.keys.iter().any(|key| key == "Ctrl+Alt+C")));
    println!("Shortcuts: {} copy mode rows", rows.len());

    h.press("ctrl-shift-p");
    h.type_text("Enter Copy Mode");
    assert_eq!(
        h.workspace(|ws| ws.list_titles()[0].clone()),
        "Enter Copy Mode"
    );
    h.press("enter");
    let start = h.wait("copy cursor from the palette", |m| frame(m)?.copy_position);
    assert!(start.line > 20);
    h.press("g g");
    let top = h.wait("copy cursor on the first history row", |m| {
        let frame = frame(m)?;
        let at = frame.copy_position?;
        (at.line == 1 && frame.scrollbar.offset == 0).then(|| frame.line_text(0))
    });
    assert_eq!(top, "history-001");
    h.press("v 6 l");
    let at = h.wait("seven selected columns", |m| {
        let frame = frame(m)?;
        let at = frame.copy_position?;
        (at.column == 7 && frame.has_selection).then_some(at)
    });
    println!(
        "Badge: line {}, column {}, {:?}",
        at.line, at.column, at.selection
    );
    h.screenshot("copy-mode-selection");
    h.press("y");
    wait_clipboard(&mut h, "history");
    h.wait("yank leaves copy mode at the live view", |m| {
        let frame = frame(m)?;
        (frame.copy_position.is_none() && frame.scrollbar.at_bottom() && !frame.has_selection)
            .then_some(())
    });

    // Ctrl+Shift+C still copies an ordinary mouse selection rather than entering the mode.
    let row = h.wait("the fixture line under the live cursor", |m| {
        focused_text(m)
            .lines()
            .position(|line| line == "copy-live-tail")
    }) as u16;
    let window = h.window;
    let at = h.cx.update(|cx| {
        let ws = window.read(cx).unwrap();
        ws.focused_terminal()
            .unwrap()
            .read(cx)
            .cell_point(2, row)
            .unwrap()
    });
    let mods = Modifiers::default();
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: mods,
        click_count: 3,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: mods,
        click_count: 3,
    }));
    h.wait("a mouse selection", |m| {
        frame(m)?.has_selection.then_some(())
    });
    h.cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())));
    h.press("ctrl-shift-c");
    wait_clipboard(&mut h, "copy-live-tail");
    assert!(frame(&h.host.snapshot()).unwrap().copy_position.is_none());

    h.press("ctrl-alt-c");
    let start = h.wait("Linux copy mode shortcut", |m| frame(m)?.copy_position);
    h.press("x k");
    h.wait("k moves one line without typing x into the shell", |m| {
        (frame(m)?.copy_position?.line + 1 == start.line).then_some(())
    });
    h.press("escape");
    h.wait("Escape restores live typing", |m| {
        frame(m)?.copy_position.is_none().then_some(())
    });
    h.type_text("echo copy-keyboard-returned");
    h.press("enter");
    h.wait("the shell answered after Escape", |m| {
        focused_text(m)
            .contains("\ncopy-keyboard-returned\n")
            .then_some(())
    });

    if std::env::var_os("REXIA_TEST_COPY_MODE_LUA").is_some() {
        assert!(
            rows.iter()
                .any(|r| r.keys.iter().any(|key| key == "copy: Z") && r.config.is_some())
        );
        h.press("ctrl-alt-shift-c");
        h.wait("init.lua enters copy mode", |m| frame(m)?.copy_position);
        h.press("g g q z");
        h.wait("init.lua blocks q and binds z to right", |m| {
            let at = frame(m)?.copy_position?;
            (at.line == 1 && at.column == 2).then_some(())
        });
        println!("Verified init.lua copy/ bind, unbind, and client.mode.enter");
        h.press("escape");
    }
}
