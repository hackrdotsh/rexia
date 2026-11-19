//! Where in-app notices float, against a real Rex server named by REXIA_TEST_SERVER.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::settings::NoticePosition;
use serde_json::Value;

const WIDTH: f32 = 1280.;
const HEIGHT: f32 = 800.;

#[test]
fn notices_float_where_the_setting_says() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-notice");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, WIDTH, HEIGHT);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let window = h.window;

    for position in NoticePosition::ALL {
        h.cx.update(|cx| {
            window
                .update(cx, |ws, _, cx| {
                    ws.notice = Some(format!("A notice at {}", position.label()));
                    ws.update_settings(|s| s.notifications.notice_position = position, cx);
                })
                .unwrap()
        });
        h.pump();
        let bounds = h.wait_ui("the notice to draw", |ws| ws.notice_bounds());
        let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (w, hgt) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        assert!(w < 330., "a one-line notice should fit its contents: {w}");
        let center = x + w / 2.;
        println!(
            "{:>13}: x {x:.0}..{:.0}, y {y:.0}..{:.0}",
            position.label(),
            x + w,
            y + hgt
        );
        // Top notices sit below the 48px toolbar; bottom ones near the bottom edge.
        if position.top() {
            assert!(y > 48. && y < 80., "{position:?} top at {y}");
        } else {
            assert!(
                y + hgt > HEIGHT - 32. && y + hgt <= HEIGHT,
                "{position:?} bottom at {}",
                y + hgt
            );
        }
        match position.column() {
            0 => assert!(x < 32., "{position:?} left at {x}"),
            2 => assert!(
                x + w > WIDTH - 32. && x + w <= WIDTH,
                "{position:?} right at {}",
                x + w
            ),
            _ => assert!(
                (center - WIDTH / 2.).abs() < 2.,
                "{position:?} centered at {center}"
            ),
        }
        if position == NoticePosition::TopRight {
            h.screenshot("notice-top-right");
        }
    }

    // The setting's diagram, on the Notifications page.
    h.run("client.settings", Value::Null);
    h.press("tab");
    h.press("tab");
    h.screenshot("settings-notice-position");
    drop(sessions);
}
