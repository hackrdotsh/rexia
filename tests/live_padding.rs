//! Program colors filling the padding, against a real Rex server named by
//! REXIA_TEST_SERVER: a shell clears its screen red, then a full-screen program paints blue.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::settings::{PaddingFill, Settings};
use std::time::{Duration, Instant};

const RED: u32 = 0xc82828;
const BLUE: u32 = 0x283cc8;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

/// The focused pane's padding colors and header color, once `check` accepts them.
fn wait_fill(
    h: &mut Harness,
    what: &str,
    check: impl Fn(&[u32], Option<u32>) -> bool,
) -> (Vec<u32>, Option<u32>) {
    let window = h.window;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        h.pump();
        let fill =
            h.cx.update(|cx| window.read(cx).unwrap().focused_padding_fill(cx));
        if let Some((colors, header)) = fill.clone()
            && check(&colors, header)
        {
            return (colors, header);
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {fill:06x?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn padding_fills_with_program_colors() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-padding");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    let directory = tempfile::tempdir().unwrap();
    let prompt = directory.path().join("prompt.sh");
    std::fs::write(&prompt, concat!(
        "printf '\\033[0m\\033[2J\\033[HLEFT \\033[48;2;200;40;40m PROMPT_COLOR_TEST \\033[0m RIGHT'\n",
        "printf '\\033[999;1HLEFT \\033[48;2;200;40;40m PROMPT_COLOR_TEST \\033[0m RIGHT'\n",
        "read -r advance\n",
        "printf '\\033[48;2;200;40;40m\\033[2J\\033[0m'\n",
        "read -r advance\n",
        "printf '\\033[?1049h\\033[48;2;40;60;200m\\033[2J\\033[H'\n",
        "sleep 60\n"
    )).unwrap();
    settings(&mut h, |s| {
        s.terminal.padding_fill = PaddingFill::Always;
        s.terminal.padding_x = 16.;
        s.terminal.padding_y = 16.;
    });
    h.type_text(&format!("sh {}", prompt.display()));
    h.press("enter");
    h.wait("mixed primary-screen prompt", |model| {
        focused_text(model)
            .contains("PROMPT_COLOR_TEST")
            .then_some(())
    });
    h.screenshot("padding-mixed-prompt");
    let (prompt_colors, _) =
        h.cx.update(|cx| h.window.read(cx).unwrap().focused_padding_fill(cx))
            .unwrap();
    assert!(
        !prompt_colors.contains(&RED),
        "prompt color leaked into padding: {prompt_colors:06x?}"
    );
    // Advance the waiting script, rather than racing the shell's redraw after Ctrl+C.
    h.press("enter");
    settings(&mut h, |s| s.terminal.padding_fill = PaddingFill::Always);
    let (always, _) = wait_fill(&mut h, "red padding", |colors, _| colors.contains(&RED));
    println!("shell, Always: {always:06x?}");
    // Automatic waits for a full-screen program.
    settings(&mut h, |s| s.terminal.padding_fill = PaddingFill::Automatic);
    let (automatic, _) = wait_fill(&mut h, "no fill", |colors, _| colors.is_empty());
    println!("shell, Automatic: {automatic:06x?}");

    // The next phase enters the alternate screen, filling the padding and header blue.
    h.press("enter");
    let (colors, header) = wait_fill(&mut h, "blue padding", |colors, header| {
        colors == [BLUE] && header == Some(BLUE)
    });
    println!("full screen, Automatic: padding {colors:06x?}, header {header:06x?}");
    h.screenshot("padding-fill");
    // Off leaves the padding and the header alone.
    settings(&mut h, |s| s.terminal.padding_fill = PaddingFill::Off);
    let (off, header) = wait_fill(&mut h, "no fill", |colors, _| colors.is_empty());
    println!("full screen, Off: padding {off:06x?}, header {header:06x?}");
    assert_eq!(header, None);
    drop(sessions);
}
