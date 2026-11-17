//! A small contact sheet of sourced logos and terminal-colored shell variants.
mod common;

use gpui::{
    AssetSource, Context, HeadlessAppContext, IntoElement, Render, SharedString, Window, div,
    prelude::*, px, size,
};
use rexia::ui::{
    icons::Assets,
    program_icons::tile,
    theme::{Theme, color},
};
use std::{borrow::Cow, sync::Arc};

const LOGOS: &[(&str, &[u8])] = &[
    (
        "claude-code",
        include_bytes!("../assets/programs/claude-code.svg"),
    ),
    ("codex", include_bytes!("../assets/programs/codex.svg")),
    ("git", include_bytes!("../assets/programs/git.svg")),
    ("python", include_bytes!("../assets/programs/python.svg")),
    (
        "co-codes",
        include_bytes!("../assets/programs/co-codes.svg"),
    ),
];

struct PreviewAssets;
impl AssetSource for PreviewAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path
            .strip_prefix("icons/sourced/")
            .and_then(|path| path.strip_suffix(".svg"))
        {
            return Ok(LOGOS
                .iter()
                .find(|(id, _)| *id == name)
                .map(|(_, bytes)| Cow::Borrowed(*bytes)));
        }
        Assets.load(path)
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Assets.list(path)
    }
}

struct Preview;
impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::from_noctalia(&rexia::noctalia::load());
        let shell = theme.terminal;
        let examples = [
            (
                "Shell",
                "programs/shell",
                shell.background,
                Some(shell.foreground),
            ),
            (
                "Shell · caffeinate",
                "programs/shell-caffeinate",
                shell.background,
                Some(shell.foreground),
            ),
            (
                "Shell · log",
                "programs/shell-log",
                shell.background,
                Some(shell.foreground),
            ),
            ("Claude Code", "sourced/claude-code", 0xb96246, None),
            ("Codex", "sourced/codex", 0x167d68, None),
            ("Git", "sourced/git", 0xc03d2c, None),
            ("Python", "sourced/python", 0x3776ab, None),
            ("co", "sourced/co-codes", 0x8758ff, Some(0xffffff)),
            ("Gojo", "programs/gojo", 0x4a6572, Some(0xffffff)),
        ];
        div()
            .size_full()
            .bg(color(0x101114))
            .text_color(color(0xf2f2f4))
            .font_family("DM Sans")
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .text_size(px(18.))
                    .child("Program icons · flat color preview"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .children(examples.into_iter().map(|(label, icon, background, ink)| {
                        div()
                            .w(px(207.))
                            .h(px(174.))
                            .rounded(px(16.))
                            .bg(color(0x191a1e))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(15.))
                            .child(tile::render(icon, 74., background, ink))
                            .child(div().text_size(px(15.)).child(label))
                    })),
            )
    }
}

#[test]
fn preview_sourced_logos_and_terminal_colored_shells() {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
        Arc::new(PreviewAssets),
        gpui_platform::current_headless_renderer,
    );
    let window = cx
        .open_window(size(px(700.), px(638.)), |_, cx| cx.new(|_| Preview))
        .unwrap();
    cx.run_until_parked();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let path = common::shot_dir().join("sourced-program-icons-co-gojo.png");
    image.save(&path).unwrap();
    println!("saved {}", path.display());
}
