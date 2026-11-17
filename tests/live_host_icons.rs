mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{AppContext as _, px, size};
use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::GifEncoder};
use rexia::settings::HostConfig;
use std::{
    fs::File,
    time::{Duration, Instant},
};

#[test]
fn sidebar_host_gif_only_animates_for_the_selected_host_in_the_focused_window() {
    let Some(address) = common::server() else {
        return;
    };
    let sessions = Sessions::create(&common::connect(&address), "rexia-host-icons");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("host.gif");
    let mut gif = GifEncoder::new(File::create(&path).unwrap());
    for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
        gif.encode_frame(Frame::from_parts(
            RgbaImage::from_pixel(24, 24, Rgba(color)),
            0,
            0,
            Delay::from_numer_denom_ms(100, 1),
        ))
        .unwrap();
    }
    drop(gif);

    let mut settings = common::test_settings(&address, &sessions.ids[0]);
    settings.hosts.push(HostConfig {
        id: "other".into(),
        label: "Other".into(),
        endpoint: format!("unix://{}", directory.path().join("absent.sock").display()),
    });
    for id in ["test", "other"] {
        settings.host_icons.insert(id.into(), path.clone());
    }
    let mut harness = Harness::open_with(settings, 1000., 700.);
    harness.wait("terminal prompt", |model| {
        (!focused_text(model).is_empty()).then_some(())
    });
    harness.activate();
    let colors = |harness: &mut Harness| {
        harness.pump();
        let image = harness
            .cx
            .capture_screenshot(harness.window.into())
            .unwrap();
        let mut icons = Vec::new();
        let x = image.width() / 40;
        let mut previous = None;
        for y in 0..image.height() {
            let pixel = image.get_pixel(x, y).0;
            let color = match pixel {
                [255, 0, 0, 255] => Some('r'),
                [0, 255, 0, 255] => Some('g'),
                _ => None,
            };
            if let Some(color) = color.filter(|_| previous != color) {
                icons.push(color);
            }
            previous = color;
        }
        icons
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let visible = colors(&mut harness);
        if visible == ['g', 'r'] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "only the selected host should play its second frame: {visible:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let _other_window = harness
        .cx
        .open_window(size(px(100.), px(100.)), |window, cx| {
            window.activate_window();
            cx.new(|_| gpui::Empty)
        })
        .unwrap();
    assert_eq!(colors(&mut harness), ['r', 'r']);
}
