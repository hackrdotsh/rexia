//! Small building blocks drawn the way Noctalia draws them: outlined boxes on the surface,
//! filled with the primary color when active, corners and borders as Noctalia configures.
use super::{icons::icon, theme::Theme};
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, SharedString, Stateful, Window, div,
    prelude::*, px,
};

pub type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A square icon button with Noctalia's button border.
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &'static str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let size = px(30.);
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(theme.radius_small())
        .when(theme.button_borders, |d| {
            d.border(theme.border_width).border_color(theme.border)
        })
        .text_color(theme.text)
        .hover(|d| d.bg(theme.overlay(0.08)))
        .active(|d| d.bg(theme.overlay(0.14)))
        .cursor_pointer()
        .child(icon(name).size(px(16.)).text_color(theme.text))
        .on_click(on_click)
}

pub const SMALL_ICON_BUTTON_WIDTH: f32 = 22.;

/// A borderless icon button for dense rows such as pane headers and tabs.
pub fn small_icon_button(
    id: impl Into<ElementId>,
    name: &'static str,
    color: Hsla,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(SMALL_ICON_BUTTON_WIDTH))
        .rounded(theme.radius_small())
        .hover(|d| d.bg(theme.overlay(0.1)))
        .cursor_pointer()
        .child(icon(name).size(px(14.)).text_color(color))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_click(event, window, cx)
        })
}

/// A list row that fills with the primary color when selected, as Noctalia's lists do.
pub fn list_row(id: impl Into<ElementId>, selected: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .h(px(32.))
        .rounded(theme.radius_small())
        .cursor_pointer()
        .when(selected, |d| {
            d.bg(theme.primary).text_color(theme.on_primary)
        })
        .when(!selected, |d| {
            d.text_color(theme.text)
                .hover(|d| d.bg(theme.overlay(0.06)))
        })
}

/// A small filled dot, for activity.
pub fn dot(color: Hsla, size: f32) -> Div {
    div().flex_none().size(px(size)).rounded_full().bg(color)
}

/// A muted section heading.
pub fn heading(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .px(px(6.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(12.))
        .text_color(theme.text_muted)
        .child(text.into())
}

/// A bold section title, like "Workspaces" in Noctalia's panels.
pub fn title(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(15.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(theme.text)
        .child(text.into())
}

/// A card: Noctalia's surface container with its card border.
pub fn card(theme: &Theme) -> Div {
    div()
        .bg(theme.surface)
        .rounded(theme.radius())
        .when(theme.card_borders, |d| {
            d.border(theme.border_width).border_color(theme.border)
        })
}

pub fn into_any(element: impl IntoElement) -> AnyElement {
    element.into_any_element()
}

/// The tooltip shown last, as "Label" or "Label  Keys", so tests can see what appeared.
#[derive(Default)]
pub struct ShownTooltip(pub Option<String>);

impl gpui::Global for ShownTooltip {}

/// A button's tooltip in Noctalia's popup style: its name, then its shortcut when it has one.
pub struct Tooltip {
    label: SharedString,
    keys: Option<SharedString>,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        div().pl(px(8.)).pt(px(6.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .font_family(theme.ui_font.clone())
                .text_size(px(12.))
                .text_color(theme.text)
                .bg(theme.surface_high)
                .rounded(theme.radius_small())
                .when(theme.popup_borders, |d| {
                    d.border(theme.border_width).border_color(theme.border)
                })
                .when(theme.popup_shadows, |d| d.shadow_md())
                .child(self.label.clone())
                .children(
                    self.keys
                        .clone()
                        .map(|keys| div().text_color(theme.text_muted).child(keys)),
                ),
        )
    }
}

/// Builds a tooltip for `.tooltip(...)`, recording it as the one shown.
pub fn tooltip(
    label: impl Into<SharedString>,
    keys: Option<String>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let label = label.into();
    let keys: Option<SharedString> = keys.map(Into::into);
    move |_, cx| {
        let shown = match &keys {
            Some(keys) => format!("{label}  {keys}"),
            None => label.to_string(),
        };
        cx.set_global(ShownTooltip(Some(shown)));
        cx.new(|_| Tooltip {
            label: label.clone(),
            keys: keys.clone(),
        })
        .into()
    }
}
