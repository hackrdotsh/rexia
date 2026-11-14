//! Characters rexia draws itself, against a real Rex server named by REXIA_TEST_SERVER:
//! braille and block elements print, reach the screen, and are saved as a screenshot (the
//! drawing itself is checked by `sprites`' unit tests).
mod common;

use common::{Harness, Sessions, focused_text};

#[test]
fn braille_and_blocks_draw_in_the_terminal() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-glyphs");
    let mut h = Harness::open(&address, &sessions.ids[0], 900., 420.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text(
        "clear; printf '%s\\n' 'braille ⠁⠃⠇⡇⣇⣧⣷⣿ ⣀⣤⣶⣿⢸⡸' 'blocks  ▁▂▃▄▅▆▇█ ▏▎▍▌▋▊▉ ░▒▓ ▀▐▖▗▘▙▚▛▜▝▞▟'",
    );
    h.press("enter");
    let screen = h.wait("the glyphs", |m| {
        let text = focused_text(m);
        text.contains("braille ⠁").then_some(text)
    });
    assert!(screen.contains("⣀⣤⣶⣿") && screen.contains("▁▂▃▄▅▆▇█"));
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
    println!("saved {}", h.screenshot("glyphs").display());
    drop(sessions);
}
