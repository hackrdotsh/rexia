//! Background alpha through the actual workspace, with a real Rex terminal.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::prelude::*;
use gpui::{Pixels, Point};
use rexia::settings::{FadeInactive, PaddingFill, Settings};
use serde_json::json;
use std::sync::Arc;

const BACKDROP: u32 = 0x40a060;

struct Backdrop(Box<dyn gpui::PlatformHeadlessRenderer>);

struct OtherWindow;

impl gpui::Render for OtherWindow {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

impl Backdrop {
    fn scene(scene: &gpui::Scene, size: gpui::Size<gpui::DevicePixels>) -> gpui::Scene {
        let bounds = gpui::Bounds::new(
            gpui::Point::default(),
            gpui::size(
                gpui::ScaledPixels(size.width.0 as f32),
                gpui::ScaledPixels(size.height.0 as f32),
            ),
        );
        let mut composed = gpui::Scene::default();
        composed.insert_primitive(gpui::Quad {
            bounds,
            content_mask: gpui::ContentMask { bounds },
            background: gpui::rgb(BACKDROP).into(),
            ..Default::default()
        });
        composed.replay(0..scene.len(), scene);
        composed.finish();
        composed
    }
}

impl gpui::PlatformHeadlessRenderer for Backdrop {
    fn render_scene_to_image(
        &mut self,
        scene: &gpui::Scene,
        size: gpui::Size<gpui::DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.0
            .render_scene_to_image(&Self::scene(scene, size), size)
    }

    fn render_scene(
        &mut self,
        scene: &gpui::Scene,
        size: gpui::Size<gpui::DevicePixels>,
    ) -> anyhow::Result<()> {
        self.0.render_scene(&Self::scene(scene, size), size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.0.sprite_atlas()
    }
}

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn pixel(h: &mut Harness, at: Point<Pixels>) -> [u8; 4] {
    h.pump();
    let image = h.cx.capture_screenshot(h.window.into()).unwrap();
    let scale = image.width() as f32 / 1100.;
    image
        .get_pixel(
            (f32::from(at.x) * scale) as u32,
            (f32::from(at.y) * scale) as u32,
        )
        .0
}

fn background(pixel: [u8; 4], rgb: u32, opacity: f32) {
    let channels = |rgb: u32| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    for ((actual, fg), bg) in pixel[..3].iter().zip(channels(rgb)).zip(channels(BACKDROP)) {
        let expected = (f32::from(fg) * opacity + f32::from(bg) * (1. - opacity)).round() as u8;
        assert!(
            actual.abs_diff(expected) <= 2,
            "{pixel:?}: expected {rgb:06x} at {opacity}, channel {expected}"
        );
    }
}

#[test]
fn only_backgrounds_follow_pane_and_window_focus() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-opacity");
    let session = &sessions.ids[0];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    let mut config = common::test_settings(&address, session);
    config.interface.active_pane_opacity = 0.5;
    config.interface.inactive_pane_opacity = 0.25;
    config.terminal.padding_x = 16.;
    config.terminal.padding_y = 16.;
    let mut h = Harness::open_with_renderer(config, Some(path.clone()), 1100., 700., || {
        Ok(gpui_platform::current_headless_renderer()?
            .map(|r| Box::new(Backdrop(r)) as Box<dyn gpui::PlatformHeadlessRenderer>))
    });
    let first = h.wait("prompt", |m| {
        (!focused_text(m).is_empty())
            .then(|| m.view()?.active_window()?.focused_block_id.clone())
            .flatten()
    });
    h.activate();
    h.run("pane.split.right", json!(null));
    let second = h.wait("split", |m| {
        let id = m.view()?.active_window()?.focused_block_id.clone()?;
        (id != first && !focused_text(m).is_empty()).then_some(id)
    });
    h.call_from(
        &control,
        "session.focus_block",
        json!({"session_id": session, "block_id": first}),
    )
    .unwrap();
    h.wait("first focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(&first)).then_some(())
    });
    let default_bg = h.cx.update(|cx| {
        h.window
            .read(cx)
            .unwrap()
            .focused_terminal()
            .unwrap()
            .read(cx)
            .background()
            .unwrap()
    });
    let fixture = directory.path().join("colors.sh");
    std::fs::write(&fixture, format!(concat!(
        r"printf '\033[2J\033[H\033[3;1H\033[38;2;255;0;0;48;2;0;0;255m▀▀▀▀▀▀▀▀▀▀\033[0m\033[4;1H\033[48;2;{};{};{}m          \033[0m\033[5;1HOPACITY_READY'",
        "\nsleep 60\n"
    ), (default_bg >> 16) & 255, (default_bg >> 8) & 255, default_bg & 255)).unwrap();
    h.type_text(&format!("sh {}", fixture.display()));
    h.press("enter");
    h.wait("color fixture", |m| {
        focused_text(m).contains("OPACITY_READY").then_some(())
    });
    h.screenshot("pane-opacity-probe");
    let row = h.wait("half-block row", |m| {
        let frame = m.panes.get(&first)?.snapshot().frame?;
        (0..frame.rows).find(|r| frame.line_text(*r as usize).trim() == "▀▀▀▀▀▀▀▀▀▀")
    });
    let window = h.window;
    let (center, cell, blank, padding, bg) = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        let pane = pane.read(cx);
        let center = pane.cell_point(4, row).unwrap();
        let cell = pane.cell_size().unwrap();
        let blank = pane.cell_point(4, 6).unwrap();
        let first = pane.cell_point(0, 0).unwrap();
        (
            center,
            cell,
            blank,
            Point::new(first.x - cell.width / 2. - gpui::px(8.), blank.y),
            pane.background().unwrap(),
        )
    });
    let upper = Point::new(center.x, center.y - cell.height * 0.3);
    let lower = Point::new(center.x, center.y + cell.height * 0.3);
    let explicit_default = Point::new(center.x, center.y + cell.height);
    let red = pixel(&mut h, upper);
    background(red, 0xff0000, 1.);
    assert!(red[0] > 245 && red[2] < 5, "foreground stays red: {red:?}");
    background(pixel(&mut h, lower), 0x0000ff, 1.);
    background(pixel(&mut h, blank), bg, 0.5);
    background(pixel(&mut h, explicit_default), bg, 0.5);
    background(pixel(&mut h, padding), bg, 0.5);
    h.screenshot("pane-opacity-active");

    let other =
        h.cx.open_window(gpui::size(gpui::px(100.), gpui::px(100.)), |window, cx| {
            window.activate_window();
            cx.new(|_| OtherWindow)
        })
        .unwrap();
    background(pixel(&mut h, lower), 0x0000ff, 1.);
    background(pixel(&mut h, blank), bg, 0.25);
    background(pixel(&mut h, explicit_default), bg, 0.25);
    h.activate();
    background(pixel(&mut h, lower), 0x0000ff, 1.);
    background(pixel(&mut h, blank), bg, 0.5);
    h.cx.update(|cx| {
        other
            .update(cx, |_, window, _| window.remove_window())
            .unwrap()
    });

    h.call_from(
        &control,
        "session.focus_block",
        json!({"session_id": session, "block_id": second}),
    )
    .unwrap();
    h.wait("second focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(&second)).then_some(())
    });
    background(pixel(&mut h, lower), 0x0000ff, 1.);
    background(pixel(&mut h, blank), bg, 0.25);
    background(pixel(&mut h, upper), 0xff0000, 1.);
    settings(&mut h, |s| s.interface.fade_inactive = FadeInactive::Both);
    let dim_blue =
        rexia::ui::theme::color(bg).blend(rexia::ui::theme::color(0x0000ff).opacity(0.6));
    let dim_blue = gpui::Rgba::from(dim_blue);
    let dim_blue = ((dim_blue.r * 255.).round() as u32) << 16
        | ((dim_blue.g * 255.).round() as u32) << 8
        | (dim_blue.b * 255.).round() as u32;
    background(pixel(&mut h, lower), dim_blue, 1.);
    let dimmed = pixel(&mut h, upper);
    assert_eq!(dimmed[3], 255);
    assert!(dimmed[0] < red[0] - 50, "dimming still works: {dimmed:?}");
    h.screenshot("pane-opacity-inactive-dimmed");

    settings(&mut h, |s| s.interface.inactive_pane_opacity = 0.);
    background(pixel(&mut h, lower), dim_blue, 1.);
    background(pixel(&mut h, blank), bg, 0.);
    background(pixel(&mut h, explicit_default), bg, 0.);
    background(pixel(&mut h, padding), bg, 0.);
    assert_eq!(pixel(&mut h, upper), dimmed);
    settings(&mut h, |s| s.interface.inactive_pane_opacity = 1.);
    background(pixel(&mut h, lower), dim_blue, 1.);
    background(pixel(&mut h, blank), bg, 1.);
    let saved = Settings::load(&path).unwrap();
    assert_eq!(saved.interface.active_pane_opacity, 0.5);
    assert_eq!(saved.interface.inactive_pane_opacity, 1.);

    settings(&mut h, |s| {
        s.interface.inactive_pane_opacity = 0.5;
        s.interface.fade_inactive = FadeInactive::Off;
        s.terminal.padding_fill = PaddingFill::Always;
    });
    background(pixel(&mut h, padding), bg, 0.5);

    h.call_from(
        &control,
        "session.focus_block",
        json!({"session_id": session, "block_id": first}),
    )
    .unwrap();
    h.wait("first focused again", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(&first)).then_some(())
    });
    h.press("ctrl-c");
    let full = directory.path().join("full.sh");
    std::fs::write(
        &full,
        concat!(
            r"printf '\033[?1049h\033[48;2;0;0;255m\033[2J\033[H'",
            "\nsleep 60\n"
        ),
    )
    .unwrap();
    h.type_text(&format!("sh {}", full.display()));
    h.press("enter");
    h.wait("full-screen fill", |m| {
        m.panes
            .get(&first)?
            .snapshot()
            .frame?
            .alternate_screen
            .then_some(())
    });
    h.screenshot("pane-opacity-fullscreen");
    background(pixel(&mut h, blank), 0x0000ff, 1.);
    background(pixel(&mut h, padding), 0x0000ff, 1.);
    let header = Point::new(
        blank.x + gpui::px(140.),
        padding.y - cell.height * 6.5 - gpui::px(30.),
    );
    background(pixel(&mut h, header), 0x0000ff, 1.);
    settings(&mut h, |s| {
        s.interface.pane_headers = rexia::settings::PaneHeaders::Hover
    });
    let header_base = h.cx.update(|cx| {
        u32::from(gpui::Rgba::from(
            rexia::ui::theme::Theme::get(cx).background,
        )) >> 8
    });
    background(pixel(&mut h, header), header_base, 0.5);
    h.input(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
        position: header,
        pressed_button: None,
        modifiers: Default::default(),
    }));
    background(pixel(&mut h, header), 0x0000ff, 1.);

    for setting in [
        "interface.active_pane_opacity",
        "interface.inactive_pane_opacity",
    ] {
        h.cx.update(|cx| {
            window
                .update(cx, |ws, window, cx| {
                    ws.open_indexed_setting(setting, window, cx).unwrap();
                })
                .unwrap()
        });
        h.pump();
        h.press("ctrl-a");
        h.type_text("0.35");
        h.press("enter");
        assert_eq!(
            rexia::settings_index::value(&Settings::load(&path).unwrap(), setting),
            json!(0.35_f32)
        );
        h.press("ctrl-a");
        h.type_text("1.2");
        h.press("enter");
        assert!(h.workspace(|ws| ws.setting_error().is_some()));
        let at = h
            .workspace(|ws| ws.element_bounds(&format!("setting-reset:{setting}")))
            .unwrap()
            .center();
        h.input(gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        }));
        h.input(gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: Default::default(),
            click_count: 1,
        }));
        assert_eq!(
            h.workspace(|ws| rexia::settings_index::value(&ws.settings, setting)),
            json!(1.)
        );
        h.press("escape");
    }
}
