//! Input methods in a terminal pane, against a real Rex server named by REXIA_TEST_SERVER:
//! composing text shows at the cursor, and committed text reaches the program.
mod common;

use common::{Harness, Sessions, focused_text};

fn input_method(h: &mut Harness, text: &str, commit: bool) -> Option<gpui::Bounds<gpui::Pixels>> {
    let window = h.window;
    let text = text.to_owned();
    let bounds = h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.input_method(&text, commit, window, cx)
            })
            .unwrap()
    });
    h.pump();
    bounds
}

fn preedit(h: &mut Harness) -> Option<String> {
    let window = h.window;
    h.cx.update(|cx| window.read(cx).unwrap().focused_preedit(cx))
}

#[test]
fn input_methods_compose_at_the_cursor_and_commit_to_the_program() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-ime");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("echo ");

    // Composing shows at the cursor, and the candidate window goes there too.
    let at = input_method(&mut h, "にほんご", false).expect("cursor bounds");
    println!("composing at {at:?}");
    assert_eq!(preedit(&mut h).as_deref(), Some("にほんご"));
    assert!(f32::from(at.size.width) > 0. && f32::from(at.origin.x) > 0.);
    h.screenshot("ime-composing-cells");
    // Nothing reached the program while composing.
    assert!(
        !h.wait("the echo", |m| Some(focused_text(m)))
            .contains("にほんご")
    );

    // Committing types the text; the composition is gone.
    input_method(&mut h, "日本語-ime", true);
    assert_eq!(preedit(&mut h), None);
    h.press("enter");
    let text = h.wait("the echoed text", |m| {
        let text = focused_text(m);
        (text.matches("日本語-ime").count() >= 2).then_some(text)
    });
    println!(
        "{}",
        text.lines()
            .filter(|l| l.contains("日本語"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    drop(sessions);
}
