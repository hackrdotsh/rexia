//! Terminal font style and weight, against a real Rex server named by REXIA_TEST_SERVER:
//! the faces of an installed family come from fontconfig (DM Mono has Light, Regular and
//! Medium), and a heavier weight draws more ink than a lighter one.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::SettingsSection;
use std::collections::HashMap;

/// How far the window's pixels are from its most common color, its background, in all.
fn ink(h: &mut Harness, name: &str) -> u64 {
    h.screenshot(name);
    let image = h.cx.capture_screenshot(h.window.into()).unwrap();
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for pixel in image.pixels() {
        *counts.entry(pixel.0).or_default() += 1;
    }
    let background = counts.into_iter().max_by_key(|(_, n)| *n).unwrap().0;
    image
        .pixels()
        .map(|p| {
            (0..3)
                .map(|i| u64::from(p.0[i].abs_diff(background[i])))
                .sum::<u64>()
        })
        .sum()
}

fn weigh(h: &mut Harness, weight: u16) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(move |s| s.terminal.font_weight = Some(weight), cx)
            })
            .unwrap()
    });
    h.pump();
}

#[test]
fn font_faces_and_weight_draw() {
    let Some(address) = common::server() else {
        return;
    };
    let listed = std::process::Command::new("fc-list")
        .args(["DM Mono", "family"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if !listed {
        eprintln!("skipped: DM Mono isn't installed");
        return;
    }
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-font-weight");
    let mut settings = common::test_settings(&address, &sessions.ids[0]);
    settings.terminal.font_family = Some("DM Mono".into());
    settings.terminal.font_size = Some(16.);
    let mut h = Harness::open_with(settings, 1000., 600.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("printf 'WMWMWMWMWMWMWMWMWMWMWMWMWMWM\\n%.0s' 1 2 3 4 5 6 7 8");
    h.press("enter");
    h.wait("the text", |m| {
        (focused_text(m)
            .matches("WMWMWMWMWMWMWMWMWMWMWMWMWMWM")
            .count()
            >= 8)
            .then_some(())
    });

    let faces: Vec<_> = h.workspace(|ws| {
        ws.font_faces()
            .into_iter()
            .map(|f| (f.name, f.weight))
            .collect()
    });
    println!("DM Mono faces: {faces:?}");
    assert_eq!(
        faces,
        [
            ("Light".to_owned(), 300),
            ("Regular".to_owned(), 400),
            ("Medium".to_owned(), 500)
        ]
    );

    weigh(&mut h, 300);
    let light = ink(&mut h, "font-weight-light");
    weigh(&mut h, 400);
    let regular = ink(&mut h, "font-weight-regular");
    weigh(&mut h, 500);
    let medium = ink(&mut h, "font-weight-medium");
    println!("ink: Light {light}, Regular {regular}, Medium {medium}");
    assert!(light < regular, "Light should draw less ink than Regular");
    assert!(regular < medium, "Regular should draw less ink than Medium");

    // Settings › Terminal shows the faces, with Light chosen, and the weight.
    weigh(&mut h, 300);
    h.run("client.settings", serde_json::json!({}));
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.show_settings_section(SettingsSection::Terminal, window, cx)
            })
            .unwrap()
    });
    h.pump();
    h.screenshot("font-weight-settings");
    drop(sessions);
}
