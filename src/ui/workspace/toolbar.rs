//! The toolbar's layout and visibility: tabs above the panes or down a sidebar on either
//! side, a toolbar that always shows, shows when the pointer reaches the top of the window,
//! or stays hidden until revealed, and pane headers that can be revealed the same way.
use super::Workspace;
use crate::{
    settings::{PaneHeaders, ToolbarBehavior, ToolbarLayout},
    ui::{
        components::{icon_button, title},
        theme::Theme,
    },
};
use gpui::{AnyElement, Bounds, Context, Pixels, Point, div, prelude::*, px};
use std::time::{Duration, Instant};

/// The toolbar's height, above the panes or atop the sidebar.
pub const TOOLBAR_HEIGHT: f32 = 48.;
/// How close to the window's top edge the pointer must come to reveal a hover toolbar.
const HOT_EDGE: f32 = 6.;
/// How long a revealed toolbar or pane header stays without the pointer on it.
const REVEAL_FOR: Duration = Duration::from_secs(4);

/// What the toolbar and pane headers are doing right now.
#[derive(Default)]
pub struct Reveal {
    /// A toolbar that isn't always shown is showing.
    pub toolbar: bool,
    /// The reveal keeps it until this moment, whatever the pointer does.
    pub until: Option<Instant>,
    /// The pane whose header Reveal Pane Header is showing.
    pub header: Option<String>,
    pub pointer: Point<Pixels>,
    _toolbar_timer: Option<gpui::Task<()>>,
    _header_timer: Option<gpui::Task<()>>,
}

impl Workspace {
    pub fn toolbar_visible(&self) -> bool {
        self.settings.interface.toolbar_behavior == ToolbarBehavior::Always || self.reveal.toolbar
    }

    /// A toolbar that isn't always shown floats over the panes instead of moving them, so
    /// showing it doesn't resize every terminal.
    pub(super) fn toolbar_floats(&self) -> bool {
        self.settings.interface.toolbar_behavior != ToolbarBehavior::Always
    }

    /// The panes' area in window coordinates, from the last paint.
    pub fn pane_area(&self) -> Bounds<Pixels> {
        self.area
    }

    /// Whether `block`'s header shows, given the header setting and any reveal.
    pub fn header_shown(&self, block: &str) -> bool {
        self.settings.interface.pane_headers != PaneHeaders::Hidden
            || self.reveal.header.as_deref() == Some(block)
    }

    pub(super) fn header_revealed(&self, block: &str) -> bool {
        self.reveal.header.as_deref() == Some(block)
    }

    /// The pointer moved: a hover toolbar shows at the window's top edge and hides once the
    /// pointer leaves it.
    pub(super) fn track_pointer(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.reveal.pointer = at;
        let behavior = self.settings.interface.toolbar_behavior;
        if behavior == ToolbarBehavior::Always {
            return;
        }
        if behavior == ToolbarBehavior::Hover && at.y < px(HOT_EDGE) && !self.reveal.toolbar {
            self.reveal.toolbar = true;
            self.reveal.until = None;
            cx.notify();
        } else if self.reveal.toolbar
            && at.y > px(TOOLBAR_HEIGHT + 12.)
            && self
                .reveal
                .until
                .is_none_or(|until| Instant::now() >= until)
        {
            self.reveal.toolbar = false;
            cx.notify();
        }
    }

    /// `client.toolbar.reveal`: show a hidden toolbar for a moment, or hide it again.
    pub fn reveal_toolbar(&mut self, cx: &mut Context<Self>) {
        if !self.toolbar_floats() {
            return;
        }
        if self.reveal.toolbar {
            self.reveal.toolbar = false;
            self.reveal.until = None;
            cx.notify();
            return;
        }
        self.reveal.toolbar = true;
        self.reveal.until = Some(Instant::now() + REVEAL_FOR);
        self.reveal._toolbar_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_FOR).await;
            let _ = this.update(cx, |ws, cx| {
                // Still showing while the pointer is on it.
                if ws.reveal.pointer.y > px(TOOLBAR_HEIGHT + 12.) {
                    ws.reveal.toolbar = false;
                }
                ws.reveal.until = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// `client.pane_header.reveal`: show the focused pane's header for a moment.
    pub fn reveal_pane_header(&mut self, cx: &mut Context<Self>) {
        if self.settings.interface.pane_headers == PaneHeaders::Always {
            return;
        }
        let focused = self
            .model()
            .view()
            .and_then(|v| v.active_window()?.focused_block_id.clone());
        if focused.is_some() && focused == self.reveal.header {
            self.reveal.header = None;
            cx.notify();
            return;
        }
        self.reveal.header = focused;
        self.reveal._header_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_FOR).await;
            let _ = this.update(cx, |ws, cx| {
                ws.reveal.header = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// `client.tab_style.toggle`: tabs above the panes, or down the side last used.
    pub fn toggle_tab_style(&mut self, cx: &mut Context<Self>) {
        let interface = &mut self.settings.interface;
        if interface.toolbar_layout.vertical() {
            interface.vertical_side = interface.toolbar_layout;
            interface.toolbar_layout = ToolbarLayout::Above;
        } else {
            interface.toolbar_layout = match interface.vertical_side {
                ToolbarLayout::Right => ToolbarLayout::Right,
                _ => ToolbarLayout::Left,
            };
            interface.sidebar = true;
        }
        self.save_settings();
        cx.notify();
    }

    /// The toolbar atop a vertical sidebar: the session's name and its buttons.
    pub(super) fn render_side_toolbar(
        &self,
        session_label: String,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("side-toolbar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(TOOLBAR_HEIGHT))
            .px(px(10.))
            .bg(theme.background)
            .border_b(theme.border_width)
            .border_color(theme.border)
            .child(
                icon_button(
                    "toggle-sidebar",
                    "layout-sidebar",
                    theme,
                    cx.listener(|ws, _, _, cx| ws.toggle_sidebar(cx)),
                )
                .tooltip(self.tip("client.sidebar.toggle", None)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title(session_label, theme)),
            )
            .child(
                icon_button(
                    "new-tab",
                    "plus",
                    theme,
                    cx.listener(|ws, _, _, cx| ws.new_tab(cx)),
                )
                .tooltip(self.tip("client.tab.new", None)),
            )
            .child(
                icon_button(
                    "split-right",
                    "layout-columns",
                    theme,
                    cx.listener(|ws, _, _, cx| ws.split(None, true, true, cx)),
                )
                .tooltip(self.tip("pane.split.right", None)),
            )
            .child(
                icon_button(
                    "split-down",
                    "layout-rows",
                    theme,
                    cx.listener(|ws, _, _, cx| ws.split(None, false, true, cx)),
                )
                .tooltip(self.tip("pane.split.down", None)),
            )
            .into_any_element()
    }
}
