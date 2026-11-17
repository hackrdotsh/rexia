//! Character spacing and text color corrections, against a real Rex server named by
//! REXIA_TEST_SERVER. A program prints nearly invisible text and an off-theme red.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{settings::Settings, ui::text_color};

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn drawn(h: &mut Harness, row: usize, col: usize) -> u32 {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| window.read(cx).unwrap().focused_text_color(row, col, cx))
        .expect("the cell's color")
}

#[test]
fn spacing_contrast_and_theme_palette() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-text");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Character spacing widens every cell by that many pixels.
    let window = h.window;
    let cell = |h: &mut Harness| {
        h.pump();
        h.cx.update(|cx| window.read(cx).unwrap().focused_cell_width(cx))
            .unwrap()
    };
    let narrow = cell(&mut h);
    settings(&mut h, |s| s.terminal.character_spacing = 3.);
    let wide = cell(&mut h);
    println!("cell width {narrow:.2} -> {wide:.2} with +3 px spacing");
    assert!((wide - narrow - 3.).abs() < 0.01);
    settings(&mut h, |s| s.terminal.character_spacing = 0.);

    h.type_text(r"printf '\033[38;2;32;32;32mFAINT\033[0m \033[38;2;255;95;95mRED\033[0m\n'");
    h.press("enter");
    let row = h.wait("the colored output", |m| {
        focused_text(m)
            .lines()
            .position(|l| l.starts_with("FAINT RED"))
    });
    let bg = h.wait("the frame", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        let frame = m.panes.get(&block)?.snapshot().frame?;
        Some((frame.bg, frame.fg, frame.ansi))
    });
    let (background, foreground, ansi) = bg;

    // As the program asked.
    let faint = drawn(&mut h, row, 0);
    let red = drawn(&mut h, row, 6);
    println!(
        "as asked: FAINT #{faint:06x} (contrast {:.2}), RED #{red:06x}",
        text_color::contrast(faint, background)
    );
    assert_eq!((faint, red), (0x202020, 0xff5f5f));

    // Improve text contrast lifts the faint text and leaves the readable red alone.
    settings(&mut h, |s| s.terminal.improve_contrast = true);
    let lifted = drawn(&mut h, row, 0);
    println!(
        "improve contrast: FAINT #{lifted:06x} (contrast {:.2}), RED #{:06x}",
        text_color::contrast(lifted, background),
        drawn(&mut h, row, 6)
    );
    assert!(text_color::contrast(lifted, background) >= text_color::MIN_CONTRAST);
    assert_eq!(drawn(&mut h, row, 6), 0xff5f5f);

    // The theme palette maps the red to one of the theme's own colors.
    settings(&mut h, |s| {
        s.terminal.improve_contrast = false;
        s.terminal.theme_palette = true;
    });
    let themed = drawn(&mut h, row, 6);
    // The nearest of the theme's colors to this red depends on the palette Noctalia has
    // now, so work it out the way the terminal does: nearest, then readable.
    let mut theme_colors = ansi.to_vec();
    theme_colors.push(foreground);
    let expected = text_color::ensure_contrast(
        text_color::nearest(0xff5f5f, &theme_colors),
        background,
        text_color::MIN_CONTRAST,
    );
    println!(
        "theme palette: RED #{themed:06x}, expected #{expected:06x}, theme colors {ansi:06x?}"
    );
    assert_eq!(themed, expected, "the red became the nearest theme color");
    assert!(text_color::contrast(drawn(&mut h, row, 0), background) >= text_color::MIN_CONTRAST);
    h.screenshot("text-theme-palette");
    drop(sessions);
}
