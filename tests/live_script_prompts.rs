//! Run against an isolated Rex revision 17+ server using fixtures/script-prompts.lua.
mod common;

use common::{Harness, Sessions, focused_text, request};
use serde_json::{Value, json};
use std::sync::mpsc;

#[test]
fn scripts_ask_and_resume_without_losing_the_open_palette() {
    let Ok(address) = std::env::var("REXIA_TEST_PROMPT_SERVER") else {
        eprintln!(
            "skipped: set REXIA_TEST_PROMPT_SERVER to a Rex server using fixtures/script-prompts.lua"
        );
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-script-prompts");
    let session = &sessions.ids[0];
    let mut h = Harness::open(&address, session, 900., 500.);
    h.wait("the shell on a revision that supports prompts", |m| {
        (m.api_revision >= 17 && !focused_text(m).trim().is_empty()).then_some(())
    });
    let ask = |h: &mut Harness, action: &str, title: &str| {
        // Use the connection running the UI, so a blocked host event loop fails this test.
        let host = h.host.clone();
        let payload = json!({"action": action, "origin": "palette", "session_id": session});
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(request(&host, "client.action", payload));
        });
        h.wait_ui(title, |ws| {
            (ws.script_prompt_title() == Some(title)).then_some(())
        });
        receive
    };
    let finish = |h: &mut Harness, receive: mpsc::Receiver<Result<Value, String>>| {
        h.wait_ui("the script prompt closes", |ws| {
            ws.script_prompt_title().is_none().then_some(())
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap()
    };

    h.run("client.palette.toggle", json!({}));
    h.type_text("copy");
    let result = ask(&mut h, "prompt_text_fixture", "Enter a note");
    h.type_text("  keep these spaces  ");
    h.press("enter");
    assert_eq!(
        finish(&mut h, result)["result"]["answer"],
        "  keep these spaces  "
    );
    let window = h.window;
    let query = h.cx.update(|cx| window.read(cx).unwrap().list_query(cx));
    assert_eq!(query.as_deref(), Some("copy"));

    let result = ask(&mut h, "prompt_pick_fixture", "Choose a file");
    h.type_text("docs");
    h.press("enter");
    assert_eq!(finish(&mut h, result)["result"]["answer"], "first");

    let result = ask(&mut h, "prompt_multiple_fixture", "Choose files");
    h.press("down enter ctrl-enter");
    assert_eq!(
        finish(&mut h, result)["result"]["answer"],
        json!(["first", "second"])
    );

    let result = ask(&mut h, "prompt_text_fixture", "Enter a note");
    h.press("escape");
    assert_eq!(finish(&mut h, result)["result"]["reason"], "cancelled");
}
