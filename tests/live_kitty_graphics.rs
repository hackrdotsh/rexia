//! Kitty graphics from a program in a pane, through a real Rex server named by
//! REXIA_TEST_SERVER: one image per transfer medium. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use std::time::Duration;

fn images(h: &mut Harness) -> Vec<(u32, gpui::Bounds<gpui::Pixels>)> {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| window.read(cx).unwrap().focused_images(cx))
}

#[test]
fn kitty_images_from_a_program_draw_in_the_pane() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-kitty");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let mediums = h.wait("the pane's terminal", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        Some(m.panes.get(&block)?.snapshot().frame?.mediums)
    });
    println!("transfer mediums besides direct: {mediums:?}");
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/kitty_images.py"
    );
    h.type_text(&format!("python3 {script}"));
    h.press("enter");
    h.wait("the program to finish", |m| {
        focused_text(m)
            .lines()
            .any(|l| l.trim() == "sent")
            .then_some(())
    });
    // The directly sent image arrives first; give the others a moment.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !images(&mut h).iter().any(|(id, _)| *id == 11) {
        assert!(
            std::time::Instant::now() < deadline,
            "the direct image never drew"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    let drawn = images(&mut h);
    for (id, area) in &drawn {
        println!("image {id}: {area:?}");
    }
    h.screenshot("kitty-graphics");
    let ids: Vec<u32> = drawn.iter().map(|(id, _)| *id).collect();
    // 8 by 2 cells: the image scales to the cells it was given.
    let (_, direct) = drawn.iter().find(|(id, _)| *id == 11).unwrap();
    let window = h.window;
    let cell =
        h.cx.update(|cx| window.read(cx).unwrap().focused_cell_width(cx))
            .unwrap();
    println!("direct image {direct:?}, cell width {cell}");
    assert!((f32::from(direct.size.width) - cell * 8.).abs() < 1.);
    // Files stay where they were, as Kitty leaves them; temporary files and shared
    // memory are deleted once read.
    for (id, medium, on) in [
        (12, "a file", mediums.file),
        (13, "a temporary file", mediums.temp_file),
        (14, "shared memory", mediums.shared_memory),
    ] {
        if on {
            assert!(ids.contains(&id), "an image sent through {medium} draws");
        }
    }
    for entry in std::fs::read_dir("/tmp").unwrap().flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("rexia-kitty-file-")
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    drop(sessions);
}
