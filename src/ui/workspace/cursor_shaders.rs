//! Settings › Terminal › Cursor's shader rows, their preview, and the Enable Cursor
//! Shaders and Change Cursor Shader actions.
use super::{
    Workspace,
    preferences::{button, row},
};
use crate::{
    settings::CursorShader,
    ui::{
        cursor_trail::{CellSpot, MIN_JUMP, Trail, paint_trail},
        theme::{Theme, color},
    },
};
use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, Hsla, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Styled, Window, canvas, div, fill, point, prelude::*, px, size,
};
use std::{cell::Cell, rc::Rc, time::Instant};

/// The preview's grid: columns, rows, and one cell's size.
const COLS: u16 = 36;
const ROWS: u16 = 6;
const CELL: (f32, f32) = (10., 20.);

/// The preview's cursor, where clicks move it, and the trail it draws.
pub struct ShaderPreview {
    pub spot: CellSpot,
    pub trail: Option<Trail>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Default for ShaderPreview {
    fn default() -> Self {
        Self {
            spot: CellSpot {
                col: 2,
                row: 1,
                width: 1,
            },
            trail: None,
            bounds: Rc::default(),
        }
    }
}

impl Workspace {
    /// Report a real caret in window coordinates during paint.
    pub(crate) fn record_focus_caret(
        window: &Window,
        cx: &App,
        focus: &FocusHandle,
        bounds: Bounds<Pixels>,
        color: Hsla,
    ) {
        if !window.is_window_active() || cx.reduce_motion() || !focus.is_focused(window) {
            return;
        }
        if let Some(workspace) = window.root::<Self>().flatten() {
            let workspace = workspace.read(cx);
            if workspace.settings.terminal.cursor_shaders
                && window.content_mask().bounds.intersects(&bounds)
            {
                workspace.focus_trails.borrow_mut().record(
                    focus,
                    bounds,
                    color,
                    workspace.settings.terminal.cursor_shader,
                );
            }
        }
    }

    pub fn cursor_focus_trail(&self) -> Option<crate::ui::cursor_trail::FocusTrail> {
        self.focus_trails.borrow().active()
    }

    pub(super) fn render_focus_trail(
        &self,
        window: &Window,
        theme: &Theme,
        cx: &App,
    ) -> Option<AnyElement> {
        if !self.settings.terminal.cursor_shaders
            || !window.is_window_active()
            || cx.reduce_motion()
        {
            *self.focus_trails.borrow_mut() = Default::default();
            return None;
        }
        let trails = self.focus_trails.clone();
        let accent = theme.primary;
        Some(
            canvas(
                |_, _, _| {},
                move |_, (), window, _| trails.borrow().paint(window, accent),
            )
            .absolute()
            .inset_0()
            .into_any_element(),
        )
    }
    /// `client.cursor_shader.toggle`: turn cursor shaders on or off.
    pub fn toggle_cursor_shaders(&mut self, cx: &mut Context<Self>) {
        self.update_settings(
            |s| s.terminal.cursor_shaders = !s.terminal.cursor_shaders,
            cx,
        );
    }

    /// Draw `shader` behind jumping cursors, turning cursor shaders on.
    pub fn choose_cursor_shader(&mut self, shader: CursorShader, cx: &mut Context<Self>) {
        self.update_settings(
            |s| {
                s.terminal.cursor_shaders = true;
                s.terminal.cursor_shader = shader;
            },
            cx,
        );
    }

    /// Move the preview's cursor to `spot`, drawing the trail from where it was.
    pub fn move_preview_cursor(&mut self, spot: CellSpot, cx: &mut Context<Self>) {
        let preview = &mut self.shader_preview;
        if preview.spot.distance(spot) >= MIN_JUMP {
            preview.trail = Some(Trail {
                from: preview.spot,
                to: spot,
                started: Instant::now(),
                shader: self.settings.terminal.cursor_shader,
            });
        }
        preview.spot = spot;
        cx.notify();
    }

    /// The preview's cursor and the trail it's drawing, for tests.
    pub fn shader_preview(&self) -> (CellSpot, Option<Trail>) {
        (self.shader_preview.spot, self.shader_preview.trail)
    }

    /// The focused pane's cursor trail, for tests.
    pub fn focused_cursor_trail(&self, cx: &gpui::App) -> Option<Trail> {
        let block = self.focused.as_ref()?;
        self.panes.get(block)?.read(cx).cursor_trail()
    }

    pub(super) fn cursor_shader_rows(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = &self.settings.terminal;
        let mut rows = vec![self.toggle_row(
            "cursor-shaders",
            "Cursor shaders",
            "Draw a trail behind the cursor when it jumps.",
            |s| s.terminal.cursor_shaders,
            |s, v| s.terminal.cursor_shaders = v,
            theme,
            cx,
        )];
        if !t.cursor_shaders {
            return rows;
        }
        let current = t.cursor_shader;
        let buttons = CursorShader::ALL.map(|shader| {
            button(
                format!("cursor-shader-{}", shader.title()),
                shader.title(),
                shader == current,
                theme,
            )
            .on_click(cx.listener(move |ws, _, _, cx| ws.choose_cursor_shader(shader, cx)))
        });
        rows.push(
            row("cursor-shader", "Shader", current.description(), theme)
                .children(self.reset_setting_control("cursor-shader", theme, cx))
                .into_any_element(),
        );
        rows.push(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .px(px(12.))
                .pb(px(8.))
                .children(buttons)
                .into_any_element(),
        );
        rows.push(self.shader_preview_element(theme, cx));
        rows
    }

    /// A small terminal: click in it to move the cursor and watch the trail.
    fn shader_preview_element(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let preview = &self.shader_preview;
        let spot = preview.spot;
        let trail = preview
            .trail
            .and_then(|trail| Some((trail, trail.progress(Instant::now())?)));
        let bounds = preview.bounds.clone();
        let background = color(theme.terminal.background);
        let cursor = color(theme.terminal.cursor);
        let text = color(theme.terminal.foreground);
        let accent = theme.primary;
        let border = theme.border;
        let (width, height) = (CELL.0 * f32::from(COLS), CELL.1 * f32::from(ROWS));
        let grid = canvas(
            {
                let bounds = bounds.clone();
                move |area, _, _| bounds.set(area)
            },
            move |area, (), window, _| {
                window.paint_quad(fill(area, background));
                let cell = |spot: CellSpot| {
                    Bounds::new(
                        area.origin
                            + point(
                                px(CELL.0 * f32::from(spot.col)),
                                px(CELL.1 * f32::from(spot.row)),
                            ),
                        size(px(CELL.0 * f32::from(spot.width)), px(CELL.1)),
                    )
                };
                // Faint rows of "text", so the trail has something to cross.
                for r in 0..ROWS {
                    let long = 8 + (r * 7) % 22;
                    window.paint_quad(fill(
                        Bounds::new(
                            area.origin + point(px(CELL.0), px(CELL.1 * f32::from(r) + 8.)),
                            size(px(CELL.0 * f32::from(long)), px(3.)),
                        ),
                        text.opacity(0.12),
                    ));
                }
                if let Some((trail, t)) = &trail {
                    paint_trail(window, trail, *t, cell, cursor, accent);
                    window.request_animation_frame();
                }
                window.paint_quad(fill(cell(spot), cursor));
            },
        )
        .w(px(width))
        .h(px(height));
        row(
            "cursor-shader-preview",
            "Preview",
            "Click to move the cursor and watch the trail.",
            theme,
        )
        .child(
            div()
                .id("cursor-shader-grid")
                .flex_none()
                .border_1()
                .border_color(border)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, event: &MouseDownEvent, _, cx| {
                        let area = bounds.get();
                        let at = event.position - area.origin;
                        let col = (f32::from(at.x) / CELL.0).clamp(0., f32::from(COLS - 1));
                        let row = (f32::from(at.y) / CELL.1).clamp(0., f32::from(ROWS - 1));
                        ws.move_preview_cursor(
                            CellSpot {
                                col: col as u16,
                                row: row as u16,
                                width: 1,
                            },
                            cx,
                        );
                    }),
                )
                .child(grid),
        )
        .into_any_element()
    }
}
