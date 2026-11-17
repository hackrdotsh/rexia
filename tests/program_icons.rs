//! Rasterize every program icon at tab size and retain dark/light contact sheets.
mod common;

use gpui::{Context, HeadlessAppContext, IntoElement, Render, Window, div, prelude::*, px, size};
use rexia::ui::{
    icons::Assets,
    program_icons::{PROGRAMS, ProgramIcon, render_icon},
    theme::{Theme, color},
};
use std::sync::Arc;

struct Gallery {
    programs: &'static [ProgramIcon],
    light: bool,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let background = color(if self.light { 0xfaf8f6 } else { 0x15181e });
        let foreground = color(if self.light { 0x252830 } else { 0xe1e3e9 });
        let theme = Theme::from_noctalia(&Default::default());
        div()
            .size_full()
            .relative()
            .bg(background)
            .children(self.programs.iter().enumerate().map(|(i, program)| {
                div()
                    .absolute()
                    .left(px((i % 4) as f32 * 240.))
                    .top(px((i / 4) as f32 * 72.))
                    .w(px(240.))
                    .h(px(72.))
                    .child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .top(px(14.))
                            .child(render_icon(program.icon, 32., foreground, &theme, None)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(56.))
                            .top(px(22.))
                            .child(render_icon(program.icon, 16., foreground, &theme, None)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(84.))
                            .top(px(20.))
                            .text_size(px(13.))
                            .text_color(foreground)
                            .child(program.name),
                    )
            }))
    }
}

#[test]
fn every_program_icon_renders_at_tab_size_on_dark_and_light_surfaces() {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    let window = cx
        .open_window(size(px(960.), px(576.)), |_, cx| {
            cx.new(|_| Gallery {
                programs: &PROGRAMS[..32],
                light: false,
            })
        })
        .unwrap();
    for light in [false, true] {
        for (page, programs) in PROGRAMS.chunks(32).enumerate() {
            cx.update(|cx| {
                window
                    .update(cx, |gallery, window, cx| {
                        gallery.programs = programs;
                        gallery.light = light;
                        window.refresh();
                        cx.notify();
                    })
                    .unwrap()
            });
            cx.run_until_parked();
            let image = cx.capture_screenshot(window.into()).unwrap();
            let scale = image.width() as f32 / 960.;
            for (i, program) in programs.iter().enumerate() {
                let x = ((i % 4) as f32 * 240. + 56.) * scale;
                let y = ((i / 4) as f32 * 72. + 22.) * scale;
                let theme = Theme::from_noctalia(&Default::default());
                let (fg, bg) =
                    rexia::ui::program_icons::colors(program.icon, &theme, None).unwrap();
                let ink_color = [(fg >> 16) as u8, (fg >> 8) as u8, fg as u8];
                let background = [(bg >> 16) as u8, (bg >> 8) as u8, bg as u8];
                let contrast: f32 = ink_color
                    .iter()
                    .zip(background)
                    .map(|(a, b)| f32::from(a.abs_diff(b)))
                    .sum();
                let ink = (((x + 2. * scale) as u32)..((x + 14. * scale) as u32))
                    .flat_map(|x| {
                        let image = &image;
                        (((y + 2. * scale) as u32)..((y + 14. * scale) as u32))
                            .map(move |y| image.get_pixel(x, y))
                    })
                    .map(|pixel| {
                        pixel.0[..3]
                            .iter()
                            .zip(background)
                            .map(|(a, b)| f32::from(a.abs_diff(b)))
                            .sum::<f32>()
                            / contrast
                    })
                    .sum::<f32>();
                let minimum_ink = (12. * scale).powi(2) * 0.05;
                assert!(
                    ink >= minimum_ink,
                    "{} has too little visible ink at 16px: {ink}, light={light}",
                    program.name
                );
            }
            let mode = if light { "light" } else { "dark" };
            let path =
                common::shot_dir().join(format!("sourced-program-icons-{mode}-{}.png", page + 1));
            image.save(&path).unwrap();
            println!("saved {}", path.display());
        }
    }
    println!(
        "Rendered all {} program icons in both modes",
        PROGRAMS.len()
    );
}
