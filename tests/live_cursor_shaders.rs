//! Cursor shaders against a real Rex server named by REXIA_TEST_SERVER: the actions, a
//! trail when the cursor jumps, and the Settings preview. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    settings::CursorShader,
    ui::{
        cursor_trail::{self, CellSpot},
        workspace::{ListKind, SettingsSection},
    },
};
use serde_json::{Value, json};

fn trail(h: &mut Harness) -> Option<cursor_trail::Trail> {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| window.read(cx).unwrap().focused_cursor_trail(cx))
}

#[test]
fn cursor_shaders_trail_jumps_and_preview() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-cursor-shaders");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 760.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Off by default: a jump draws nothing.
    h.press("enter");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(trail(&mut h).is_none());

    // Enable Cursor Shaders, then Change Cursor Shader: with no shader named it opens a
    // list of them; naming one picks it.
    h.run("client.cursor_shader.toggle", Value::Null);
    assert!(h.workspace(|ws| ws.settings.terminal.cursor_shaders));
    h.run("client.cursor_shader.change", Value::Null);
    let listed = h.workspace(|ws| (ws.list_kind(), ws.list_titles()));
    println!("Change Cursor Shader lists: {listed:?}");
    assert_eq!(listed.0, Some(ListKind::CursorShaders));
    assert_eq!(listed.1.len(), 8);
    h.press("escape");
    h.run("client.cursor_shader.change", json!({"shader": "blaze"}));
    assert_eq!(
        h.workspace(|ws| ws.settings.terminal.cursor_shader),
        CursorShader::Blaze
    );

    // A command's output moves the cursor down a few lines: that jump draws a trail.
    h.type_text("printf 'one\\ntwo\\nthree\\n'");
    h.press("enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let made = loop {
        if let Some(t) = trail(&mut h)
            && t.from.row != t.to.row
        {
            break t;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no trail after the jump"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    println!("trail: {made:?}");
    assert_eq!(made.shader, CursorShader::Blaze);
    assert!(made.to.row > made.from.row);

    // The Settings preview: a click moves its cursor and draws the trail there.
    cursor_trail::hold_progress(Some(0.3));
    h.run("client.settings", json!({}));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.show_settings_section(SettingsSection::Terminal, window, cx);
                ws.move_preview_cursor(
                    CellSpot {
                        col: 30,
                        row: 4,
                        width: 1,
                    },
                    cx,
                );
            })
            .unwrap()
    });
    h.pump();
    let (spot, preview) = h.workspace(|ws| ws.shader_preview());
    println!("preview cursor {spot:?}, trail {preview:?}");
    assert_eq!((spot.col, spot.row), (30, 4));
    assert!(preview.is_some_and(|t| t.from.col == 2 && t.shader == CursorShader::Blaze));
    // Scroll down to the cursor rows for the picture.
    h.input(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
        position: gpui::point(gpui::px(700.), gpui::px(400.)),
        pressed_button: None,
        modifiers: Default::default(),
    }));
    h.input(gpui::PlatformInput::Scroll(gpui::ScrollEvent {
        position: gpui::point(gpui::px(700.), gpui::px(400.)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-780.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    }));
    h.screenshot("cursor-shader-preview");
    h.press("escape");
    h.screenshot("cursor-shader-pane");
    cursor_trail::hold_progress(None);
    drop(sessions);
}
