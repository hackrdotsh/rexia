//! Characters rexia draws itself, against a real Rex server named by REXIA_TEST_SERVER:
//! braille, blocks, and box drawing reach the screen. Check box-drawing joins in pixels.
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

#[test]
fn box_drawing_joins_across_rows() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-box-drawing");
    let mut settings = common::test_settings(&address, &sessions.ids[0]);
    settings.terminal.font_family = Some("DejaVu Sans Mono".into());
    settings.terminal.font_size = Some(11.);
    settings.terminal.line_height = 1.5;
    let mut h = Harness::open_with(settings, 900., 420.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text(r"clear; printf '\033[38;2;255;0;255m%s\033[0m\n' '│ ┃ │' '│ ┃ │' '├───╯' '│'");
    h.press("enter");
    let row = h.wait("the box drawing", |m| {
        focused_text(m)
            .lines()
            .collect::<Vec<_>>()
            .windows(4)
            .position(|lines| lines == ["│ ┃ │", "│ ┃ │", "├───╯", "│"])
    }) as u16;
    let window = h.window;
    let (start, end, heavy, tee, corner) = h.cx.update(|cx| {
        let scale = window.update(cx, |_, w, _| w.scale_factor()).unwrap();
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        let pane = pane.read(cx);
        let cell = |col, row| {
            let pt = pane.cell_point(col, row).unwrap();
            (
                (f32::from(pt.x) * scale) as u32,
                (f32::from(pt.y) * scale) as u32,
            )
        };
        (
            cell(0, row),
            cell(0, row + 3),
            cell(2, row),
            cell(0, row + 2),
            cell(4, row + 2),
        )
    });
    let screenshot = image::open(h.screenshot("box-drawing")).unwrap().to_rgb8();
    let is_stroke = |x, y| {
        let [r, g, b] = screenshot.get_pixel(x, y).0;
        r > 200 && g < 60 && b > 200
    };
    let has_vertical = |x, y| (x - 2..=x + 2).any(|x| is_stroke(x, y));
    let height = (end.1 - start.1) / 3;
    let width = (corner.0 - tee.0) / 4;
    for y in start.1..end.1 {
        assert!(has_vertical(start.0, y), "light line has a gap at y={y}");
    }
    for y in start.1..start.1 + height {
        assert!(has_vertical(heavy.0, y), "heavy line has a gap at y={y}");
    }
    for x in tee.0..corner.0 - width / 2 + 1 {
        assert!(
            (tee.1 - 2..=tee.1 + 2).any(|y| is_stroke(x, y)),
            "horizontal line has a gap at x={x}"
        );
    }
    for y in corner.1 - height..corner.1 - width / 2 {
        assert!(
            has_vertical(corner.0, y),
            "rounded corner has a gap at y={y}"
        );
    }
}
