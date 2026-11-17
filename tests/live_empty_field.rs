mod common;

use common::{Harness, Sessions, focused_text};

#[test]
fn typing_after_select_all_in_an_empty_field_keeps_every_character() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-empty-field");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("prompt", |m| (!focused_text(m).is_empty()).then_some(()));
    h.press("ctrl-shift-p");
    h.press("ctrl-a");
    h.type_text("spr");
    let window = h.window;
    let query = h.cx.update(|cx| window.read(cx).unwrap().list_query(cx));
    assert_eq!(query.as_deref(), Some("spr"));
}
