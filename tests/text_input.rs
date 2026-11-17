use gpui::{AppContext as _, Focusable as _, HeadlessAppContext, px, size};
use rexia::ui::{icons::Assets, input::TextInput, theme::Theme};
use std::sync::Arc;

#[test]
fn focusing_an_empty_field_does_not_move_its_placeholder() {
    let mut app = HeadlessAppContext::with_platform(
        Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    let mut theme = Theme::from_noctalia(&Default::default());
    theme.input_borders = false;
    app.update(|cx| cx.set_global(theme));
    let input = app.update(|cx| cx.new(|cx| TextInput::new("Filter tabs", cx)));
    let field = input.clone();
    let window = app
        .open_window(size(px(240.), px(48.)), move |_, _| field)
        .unwrap();
    let unfocused = app.capture_screenshot(window.into()).unwrap();
    app.update_window(window.into(), |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
    })
    .unwrap();
    app.run_until_parked();
    let focused = app.capture_screenshot(window.into()).unwrap();
    // The caret is in the first 16 logical pixels; the rest of the text must be identical.
    let start = focused.width() * 16 / 240;
    assert!(
        unfocused.pixels().ne(focused.pixels()),
        "caret should appear"
    );
    for y in 0..focused.height() {
        for x in start..focused.width() {
            assert_eq!(
                unfocused.get_pixel(x, y),
                focused.get_pixel(x, y),
                "pixel at ({x}, {y}) moved"
            );
        }
    }
}
