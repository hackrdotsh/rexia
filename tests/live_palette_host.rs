//! Typing a host's address in the command palette offers to add it, against a real Rex
//! server named by REXIA_TEST_SERVER: an ssh login gets an "Add Host" row, last, that
//! opens Add Host with the address and a name filled in; a search doesn't get one.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::ListKind;

fn clear(h: &mut Harness) {
    for _ in 0..40 {
        h.press("backspace");
    }
}

#[test]
fn a_typed_address_offers_to_add_its_host() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-palette-host");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt and the keymap", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
    });

    h.press("ctrl-shift-p");
    assert_eq!(h.workspace(|ws| ws.list_kind()), Some(ListKind::Commands));

    // Wrapping reaches a row beyond the old render limit, then returns to the top.
    assert!(h.workspace(|ws| ws.list_titles().len()) > 60);
    h.press("up");
    assert!(h.workspace(|ws| ws.list_selection_visible()));
    h.press("down");
    assert!(h.workspace(|ws| ws.list_selection_visible()));
    for _ in 0..18 {
        h.press("down");
        assert!(h.workspace(|ws| ws.list_selection_visible()));
    }

    // A search: no offer.
    h.type_text("split");
    assert!(h.workspace(|ws| ws.list_selection_visible()));
    let titles = h.workspace(|ws| ws.list_titles());
    println!("'split': {} rows, last {:?}", titles.len(), titles.last());
    assert!(titles.iter().all(|t| !t.starts_with("Add Host")));

    // An ssh login: the offer, last.
    clear(&mut h);
    h.type_text("alice@studio.example");
    let titles = h.workspace(|ws| ws.list_titles());
    println!("'alice@studio.example': {titles:?}");
    assert_eq!(
        titles.last().map(String::as_str),
        Some("Add Host “alice@studio.example”…")
    );
    h.screenshot("palette-host");

    // Chosen (Up wraps to the last row): Add Host, filled in.
    h.press("up");
    h.press("enter");
    h.pump();
    let window = h.window;
    let fields =
        h.cx.update(|cx| window.read(cx).unwrap().add_host_fields(cx));
    println!("Add Host: {fields:?}");
    assert_eq!(
        fields,
        Some(("studio".to_owned(), "ssh://alice@studio.example".to_owned()))
    );
    h.screenshot("palette-host-dialog");
    h.press("escape");
    drop(sessions);
}
