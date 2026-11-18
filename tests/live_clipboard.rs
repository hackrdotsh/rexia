mod common;

use common::{Harness, Sessions, focused_text};
use gpui::ClipboardItem;
use std::time::{Duration, Instant};

#[test]
fn program_copy_updates_the_client_clipboard_and_is_not_replayed() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-program-copy");
    let mut h = Harness::open(&address, &sessions.ids[0], 950., 650.);
    h.activate();
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    for _ in 0..2 {
        h.cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("local".into())));
        h.type_text(r"printf '\033]52;c;cmVtb3RlLWNvcHk=\007'");
        h.press("enter");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            h.pump();
            if h.cx
                .update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
                .as_deref()
                == Some("remote-copy")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "OSC 52 did not reach the client clipboard"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    h.cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("newer".into())));
    h.type_text("echo clipboard-check");
    h.press("enter");
    h.wait("subsequent terminal output", |m| {
        focused_text(m)
            .contains("\nclipboard-check\n")
            .then_some(())
    });
    assert_eq!(
        h.cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .as_deref(),
        Some("newer")
    );
}
