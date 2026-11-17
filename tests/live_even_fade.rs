//! Fading an inactive pane's content, against a real Rex server named by
//! REXIA_TEST_SERVER: half-block pictures draw each pixel pair as a glyph over a cell
//! background, and fading must dim them as one picture. Red-over-blue half blocks in the
//! unfocused pane stay red on top and blue below, only dimmer, with neither showing
//! through the other.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{rex::host::HostModel, settings::FadeInactive};
use serde_json::{Value, json};

fn row_with(model: &HostModel, block: &str, needle: &str) -> Option<u16> {
    let frame = model.panes.get(block)?.snapshot().frame?;
    (0..frame.rows as usize)
        .find(|&r| frame.line_text(r).trim() == needle)
        .map(|r| r as u16)
}

fn focus(h: &mut Harness, control: &rexia::rex::host::HostHandle, session: &str, block: &str) {
    h.call_from(
        control,
        "session.focus_block",
        json!({"session_id": session, "block_id": block}),
    )
    .unwrap();
    let wanted = block.to_owned();
    h.wait("the pane focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_ref()? == &wanted).then_some(())
    });
    h.pump();
}

#[test]
fn faded_half_blocks_stay_apart() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-even-fade");
    let session = sessions.ids[0].clone();
    let mut settings = common::test_settings(&address, &session);
    settings.interface.fade_inactive = FadeInactive::Content;
    let mut h = Harness::open_with(settings, 1000., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let top = h.wait("the first pane", |m| {
        m.view()?.active_window()?.focused_block_id.clone()
    });
    h.run("pane.split.down", Value::Null);
    let bottom = h.wait("the split", |m| {
        let window = m.view()?.active_window()?;
        let focused = window.focused_block_id.clone()?;
        (focused != top).then_some(focused)
    });
    h.wait("a prompt below", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Red over blue half blocks in the top pane.
    focus(&mut h, &control, &session, &top);
    h.type_text("printf '\\033[38;2;255;0;0;48;2;0;0;255m%s\\033[0m\\n' ▀▀▀▀▀▀▀▀▀▀▀▀");
    h.press("enter");
    let block = top.clone();
    let row = h.wait("the half blocks", |m| row_with(m, &block, "▀▀▀▀▀▀▀▀▀▀▀▀"));
    let window = h.window;
    h.pump();
    let (cell, cell_size, background) = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        let pane = pane.read(cx);
        (
            pane.cell_point(4, row).unwrap(),
            pane.cell_size().unwrap(),
            pane.background().unwrap(),
        )
    });

    // Focus the bottom pane, so the top one fades.
    focus(&mut h, &control, &session, &bottom);
    h.screenshot("even-fade");
    let image = h.cx.capture_screenshot(h.window.into()).unwrap();
    let scale = image.width() as f32 / 1000.;
    let sample = |dy: f32| {
        // `cell` is the cell's middle.
        let x = f32::from(cell.x) * scale;
        let y = (f32::from(cell.y) + f32::from(cell_size.height) * (dy - 0.5)) * scale;
        image.get_pixel(x as u32, y as u32).0
    };
    let (upper, lower) = (sample(0.2), sample(0.8));
    let bg = [
        (background >> 16) as u8,
        (background >> 8) as u8,
        background as u8,
    ];
    println!(
        "row {row}, cell middle {cell:?}, size {:?}, scale {scale}",
        (cell_size.width, cell_size.height)
    );
    println!("terminal background {bg:?}; faded upper half {upper:?}, lower half {lower:?}");
    // Dimmed: not full strength, yet clearly its own color.
    assert!(
        upper[0] < 250 && upper[0] > bg[0].saturating_add(80),
        "red dims"
    );
    assert!(
        lower[2] < 250 && lower[2] > bg[2].saturating_add(80),
        "blue dims"
    );
    // Apart: the blue below doesn't show through the red, nor the red through the blue.
    assert!(
        upper[2].abs_diff(bg[2]) < 30,
        "blue shows through the red half"
    );
    assert!(
        lower[0].abs_diff(bg[0]) < 30,
        "red shows through the blue half"
    );
    drop(sessions);
}
