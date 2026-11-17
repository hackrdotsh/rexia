//! The first icon stays whole; later icons peek out behind it in narrow strips.
mod common;

use gpui::{
    Bounds, Context, HeadlessAppContext, IntoElement, Pixels, Render, Window, canvas, div,
    prelude::*, px, size,
};
use rexia::{
    settings::ShellIconTheme,
    ui::{
        icons::Assets,
        theme::{Theme, color},
        titles::Program,
        workspace::tab_icons::{TabIcon, render_icons},
    },
};
use std::sync::Arc;

const TILE: f32 = 40.;
const COLORS: [u32; 5] = [0xd03030, 0x20b060, 0x2060e0, 0xe09020, 0xb040c0];

struct Stacks {
    bounds: Vec<Option<Bounds<Pixels>>>,
}

impl Render for Stacks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::from_noctalia(&Default::default());
        let mut page = div().size_full().relative().bg(color(0x102030));
        for row in 0..10 {
            let count = row % 5 + 1;
            let colored = row < 5;
            let items: Vec<_> = COLORS[..count]
                .iter()
                .map(|bg| TabIcon {
                    program: Program::Shell,
                    colored,
                    terminal_colors: Some((0xffffff, *bg)),
                })
                .collect();
            let entity = cx.entity();
            let tracker = canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |gallery, _| gallery.bounds[row] = Some(bounds));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0();
            page = page.child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(row as f32 * 48. + 8.))
                    .flex()
                    .child(
                        div()
                            .flex_none()
                            .relative()
                            .children(render_icons(
                                &items,
                                ShellIconTheme::Default,
                                color(0xffffff),
                                color(0x102030),
                                TILE - 4.,
                                &theme,
                            ))
                            .child(tracker),
                    ),
            );
        }
        page
    }
}

#[test]
fn icon_stacks_keep_the_front_whole_and_show_small_rear_slices() {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    let window = cx
        .open_window(size(px(280.), px(488.)), |_, cx| {
            cx.new(|_| Stacks {
                bounds: vec![None; 10],
            })
        })
        .unwrap();
    cx.run_until_parked();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let scale = image.width() as f32 / 280.;
    let bounds = cx.update(|cx| window.read(cx).unwrap().bounds.clone());
    let pixel = |row: usize, x: f32, y: f32| {
        let at = bounds[row].unwrap().origin;
        image
            .get_pixel(
                ((f32::from(at.x) + x) * scale) as u32,
                ((f32::from(at.y) + y) * scale) as u32,
            )
            .0
    };
    let rgb = |color: u32| [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255];
    assert_eq!(
        pixel(2, TILE - 2., TILE / 2.),
        rgb(COLORS[0]),
        "the first icon is in front"
    );
    for (row, bounds) in bounds.iter().enumerate() {
        let count = row % 5 + 1;
        let reveal = if count == 1 {
            0.
        } else {
            0.125 + (count - 2) as f32 * 0.075
        };
        let expected = TILE * (1. + reveal);
        assert!(
            (f32::from(bounds.unwrap().size.width) - expected).abs() < 0.01,
            "row {row}: expected stack width {expected}, got {bounds:?}"
        );
    }
    assert_eq!(pixel(2, TILE + 2., TILE / 2.), rgb(COLORS[1]));
    assert_eq!(pixel(2, TILE + 6., TILE / 2.), rgb(COLORS[2]));
    assert_eq!(
        pixel(1, TILE - 2., 2.),
        rgb(COLORS[1]),
        "the rear tile keeps its rounded outline behind the front corner"
    );
    // Transparent monochrome glyphs must not show through the leading icon either.
    for row in 6..10 {
        for y in 5..35 {
            for x in 5..35 {
                assert_eq!(
                    pixel(row, x as f32, y as f32),
                    pixel(5, x as f32, y as f32),
                    "rear glyph leaked into the front at {x},{y}"
                );
            }
        }
    }
    let path = common::shot_dir().join("tab-icon-overlap.png");
    image.save(&path).unwrap();
    println!("saved {}", path.display());
}
