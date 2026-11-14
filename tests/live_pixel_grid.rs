//! The terminal grid sits on whole device pixels, as Ghostty's does, so pixel fonts stay
//! crisp: cells are a whole number of device pixels wide and tall, and the grid starts on a
//! device pixel. Against a real Rex server named by REXIA_TEST_SERVER.
mod common;

use common::{Harness, Sessions, focused_text};

#[test]
fn cells_land_on_device_pixels() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-pixel-grid");
    let mut h = Harness::open(&address, &sessions.ids[0], 1003., 611.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("printf '%s\\n' 'Total: 62.2 GiB  Used: 17.1 GiB  /nix/store/5kbv3dqx4qsny3ar5j'");
    h.press("enter");
    h.wait("the text", |m| {
        focused_text(m).contains("5kbv3dqx").then_some(())
    });
    let window = h.window;
    let (scale, points) = h.cx.update(|cx| {
        let ws = window.read(cx).unwrap();
        let pane = ws.focused_terminal().unwrap();
        let pane = pane.read(cx);
        let points: Vec<_> = [(0, 0), (1, 0), (7, 0), (0, 1), (0, 5)]
            .into_iter()
            .map(|(c, r)| pane.cell_point(c, r).unwrap())
            .collect();
        (
            window.update(cx, |_, w, _| w.scale_factor()).unwrap(),
            points,
        )
    });
    // cell_point gives a cell's middle; its corner is half a cell up and left.
    let width = f32::from(points[1].x - points[0].x) * scale;
    let height = f32::from(points[3].y - points[0].y) * scale;
    println!("cell: {width} × {height} device pixels");
    assert!((width - width.round()).abs() < 0.01 && (height - height.round()).abs() < 0.01);
    let device: Vec<(f32, f32)> = points
        .iter()
        .map(|p| {
            (
                f32::from(p.x) * scale - width / 2.,
                f32::from(p.y) * scale - height / 2.,
            )
        })
        .collect();
    println!("scale {scale}; cell corners in device pixels: {device:?}");
    for (x, y) in &device {
        assert!(
            (x - x.round()).abs() < 0.01 && (y - y.round()).abs() < 0.01,
            "off the pixel grid: {x}, {y}"
        );
    }
    println!("saved {}", h.screenshot("pixel-grid").display());
    drop(sessions);
}
