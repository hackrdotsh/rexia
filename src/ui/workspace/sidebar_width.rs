//! The sidebar's width: drag its inner edge to resize it, and the width is remembered.
//! Double-clicking the edge puts it back to the default.
use super::Workspace;
use crate::settings::ToolbarLayout;
use gpui::{
    AnyElement, Context, CursorStyle, IntoElement, MouseButton, MouseDownEvent, Pixels, Styled,
    div, prelude::*, px,
};

pub const DEFAULT: f32 = 260.;
pub const MIN: f32 = 180.;
pub const MAX: f32 = 480.;
/// Half the edge's width: it reaches this far into the sidebar and into the panes.
const HALF: f32 = 4.;

/// A drag of the sidebar's edge: where it started and the width then.
pub struct Resizing {
    start: Pixels,
    width: f32,
}

impl Workspace {
    /// The sidebar's width now, in pixels: the one being dragged to, else the saved one.
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar_drag_width
            .unwrap_or(self.settings.interface.sidebar_width)
            .clamp(MIN, MAX)
    }

    /// The handle on the sidebar's edge beside the panes: 8 pixels across the border, with
    /// a line in the accent color under the pointer and while it's dragged.
    pub(super) fn render_sidebar_edge(
        &self,
        layout: ToolbarLayout,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.global::<crate::ui::theme::Theme>().clone();
        let right_side = layout == ToolbarLayout::Right;
        let at = px(width - HALF);
        let dragging = self.sidebar_resizing.is_some();
        div()
            .id("sidebar-edge")
            .group("sidebar-edge")
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(HALF * 2.))
            .when(right_side, |d| d.right(at))
            .when(!right_side, |d| d.left(at))
            .flex()
            .justify_center()
            .cursor(CursorStyle::ResizeLeftRight)
            .child(
                div()
                    .w(px(2.))
                    .h_full()
                    .when(!dragging, |d| {
                        d.invisible()
                            .group_hover("sidebar-edge", |d| d.visible().bg(theme.primary))
                    })
                    .when(dragging, |d| d.bg(theme.primary)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        ws.sidebar_resizing = None;
                        ws.sidebar_drag_width = None;
                        ws.update_settings(|s| s.interface.sidebar_width = DEFAULT, cx);
                        return;
                    }
                    ws.sidebar_resizing = Some(Resizing {
                        start: event.position.x,
                        width: ws.sidebar_width(),
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// Follow the pointer while the edge is held; `true` when this move was the resize.
    pub(super) fn sidebar_resize_moved(
        &mut self,
        x: Pixels,
        pressed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(resizing) = &self.sidebar_resizing else {
            return false;
        };
        if !pressed {
            self.sidebar_resize_released(cx);
            return true;
        }
        // On the right, the edge is on the sidebar's left: dragging left widens it.
        let moved = f32::from(x - resizing.start);
        let moved = if self.settings.interface.toolbar_layout == ToolbarLayout::Right {
            -moved
        } else {
            moved
        };
        self.sidebar_drag_width = Some((resizing.width + moved).clamp(MIN, MAX).round());
        cx.notify();
        true
    }

    /// The edge was let go: keep the width.
    pub(super) fn sidebar_resize_released(&mut self, cx: &mut Context<Self>) -> bool {
        if self.sidebar_resizing.take().is_none() {
            return false;
        }
        if let Some(width) = self.sidebar_drag_width.take() {
            self.update_settings(|s| s.interface.sidebar_width = width, cx);
        }
        cx.notify();
        true
    }
}
