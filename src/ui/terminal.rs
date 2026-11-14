//! One terminal pane: draws the stream's latest frame on a cell grid and sends typing,
//! pasting, mouse and size changes to the Rex server. Selection, scrollback and search
//! run locally unless the program asked for mouse reports.
use super::{
    fonts::TerminalFont,
    keys,
    theme::{Theme, color},
};
use crate::rex::{
    host::{HostCommand, HostHandle, PaneCommand, PaneStatus, Scroll},
    stream::{CursorShape, Frame, SelectMode, flags},
};
use gpui::{
    App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla,
    KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollEvent, SharedString, StrikethroughStyle, TextAlign, TextRun,
    UnderlineStyle, Window, canvas, div, fill, font, point, prelude::*, px, size,
};
use serde_json::json;
use std::sync::Arc;

/// Measured once per font: the size of one cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSize {
    pub width: Pixels,
    pub height: Pixels,
}

pub struct TerminalPane {
    host: HostHandle,
    pub session_id: String,
    pub block_id: String,
    focus: FocusHandle,
    font: TerminalFont,
    pub padding: Point<Pixels>,
    cell: Option<CellSize>,
    /// The grid's top-left corner in window coordinates, from the last paint.
    origin: Point<Pixels>,
    /// The size last requested from the server.
    requested: Option<(u16, u16)>,
    /// The mouse button a program is receiving reports for.
    reporting: Option<&'static str>,
    selecting: bool,
    pub copy_on_select: bool,
    /// The pane model revision last drawn.
    pub revision: u64,
}

impl TerminalPane {
    pub fn new(
        host: HostHandle,
        session_id: String,
        block_id: String,
        font: TerminalFont,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            host,
            session_id,
            block_id,
            focus: cx.focus_handle(),
            font,
            padding: point(px(8.), px(6.)),
            cell: None,
            origin: Point::default(),
            requested: None,
            reporting: None,
            selecting: false,
            copy_on_select: false,
            revision: 0,
        }
    }

    pub fn set_font(&mut self, font: TerminalFont, cx: &mut Context<Self>) {
        if self.font != font {
            self.font = font;
            self.cell = None;
            cx.notify();
        }
    }

    fn pane(&self) -> Option<crate::rex::host::PaneHandle> {
        self.host.pane(&self.block_id)
    }

    fn frame(&self) -> Option<Arc<Frame>> {
        self.pane()?.snapshot().frame
    }

    /// The pane model changed since the last draw.
    pub fn changed(&mut self) -> bool {
        let revision = self.pane().map_or(0, |p| p.snapshot().revision);
        let changed = revision != self.revision;
        self.revision = revision;
        changed
    }

    fn terminal_font(&self) -> Font {
        let mut font = font(SharedString::from(self.font.family.clone()));
        font.features = gpui::FontFeatures::disable_ligatures();
        font
    }

    fn measure(&mut self, window: &mut Window) -> CellSize {
        if let Some(cell) = self.cell {
            return cell;
        }
        let text = window.text_system();
        let font_id = text.resolve_font(&self.terminal_font());
        let font_size = px(self.font.pixels());
        let width = text
            .advance(font_id, font_size, 'M')
            .map(|s| s.width)
            .unwrap_or(font_size * 0.6);
        // Fonts report descent as negative; the cell spans ascent plus its magnitude.
        let natural = text.ascent(font_id, font_size) + text.descent(font_id, font_size).abs();
        let height = (natural * self.font.line_height).ceil().max(px(1.));
        let cell = CellSize {
            width: width.max(px(1.)),
            height,
        };
        self.cell = Some(cell);
        cell
    }

    /// Ask the server for the grid that fits `bounds`.
    fn fit(&mut self, bounds: Bounds<Pixels>, cell: CellSize) {
        let inner_w = bounds.size.width - self.padding.x * 2.;
        let inner_h = bounds.size.height - self.padding.y * 2.;
        let cols = (inner_w / cell.width).floor().max(2.) as u16;
        let rows = (inner_h / cell.height).floor().max(1.) as u16;
        if self.requested == Some((cols, rows)) {
            return;
        }
        self.requested = Some((cols, rows));
        self.host.send(HostCommand::Resize {
            session_id: self.session_id.clone(),
            block_id: self.block_id.clone(),
            cols,
            rows,
            cell_width: f32::from(cell.width).round() as u16,
            cell_height: f32::from(cell.height).round() as u16,
        });
    }

    fn cell_at(&self, position: Point<Pixels>) -> (u16, u16) {
        let Some(cell) = self.cell else {
            return (0, 0);
        };
        let relative = position - self.origin;
        let x = (relative.x / cell.width).floor().max(0.) as u16;
        let y = (relative.y / cell.height).floor().max(0.) as u16;
        match self.frame() {
            Some(frame) => (
                x.min(frame.cols.saturating_sub(1)),
                y.min(frame.rows.saturating_sub(1)),
            ),
            None => (x, y),
        }
    }

    fn input(&self, method: &'static str, payload: serde_json::Value) {
        self.host.send(HostCommand::Input {
            session_id: self.session_id.clone(),
            block_id: self.block_id.clone(),
            method,
            payload,
            claim: true,
        });
    }

    fn block_event(&self, kind: &str, payload: serde_json::Value, claim: bool) {
        let payload = json!({"session_id": self.session_id, "block_id": self.block_id,
            "event": {"type": kind, "payload": payload}});
        if claim {
            self.input("block.event", payload);
        } else {
            self.host.send(HostCommand::Call {
                method: "block.event".into(),
                payload,
                reply: None,
            });
        }
    }

    /// Typing returns the view to the live screen and ends a selection.
    fn back_to_live(&self) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::ClearSelection);
            pane.send(PaneCommand::Scroll(Scroll::Bottom));
        }
    }

    pub fn send_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(key) = keys::key_event(&event.keystroke, event.is_held) else {
            return false;
        };
        self.back_to_live();
        self.input(
            "client.key",
            json!({"session_id": self.session_id, "block_id": self.block_id, "key": key}),
        );
        cx.notify();
        true
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.paste_text(text);
    }

    pub fn paste_text(&self, text: String) {
        if text.is_empty() {
            return;
        }
        self.back_to_live();
        self.block_event("paste", json!({"text": text}), true);
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.pane() else { return };
        cx.spawn(async move |_, cx| {
            if let Some(Some(text)) = pane.ask(PaneCommand::Copy).await
                && !text.is_empty()
            {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
            }
        })
        .detach();
    }

    pub fn select_all(&mut self) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::SelectAll);
        }
    }

    pub fn scroll(&self, scroll: Scroll) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::Scroll(scroll));
        }
    }

    pub fn scroll_pages(&self, pages: isize) {
        let rows = self.frame().map_or(24, |f| f.rows as isize);
        self.scroll(Scroll::Lines(pages * (rows - 1).max(1)));
    }

    pub fn jump_to_prompt(&self, direction: i32) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::Prompt(direction));
        }
    }

    pub fn search(&self, needle: Option<String>) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::Search(needle));
        }
    }

    pub fn search_step(&self, older: bool) {
        if let Some(pane) = self.pane() {
            pane.send(PaneCommand::SearchStep { older });
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn mouse_report(
        &mut self,
        action: &str,
        button: Option<&'static str>,
        at: (u16, u16),
        mods: &Modifiers,
    ) {
        let mut payload = json!({"action": action, "x": at.0, "y": at.1,
            "mods": keys::mods(&Modifiers { shift: false, ..*mods }, false)});
        if let Some(button) = button {
            payload["button"] = json!(button);
        }
        self.block_event("mouse", payload, action == "press");
    }

    fn tracking(&self, modifiers: &Modifiers) -> bool {
        // Shift keeps the mouse for selection, as in other terminals.
        !modifiers.shift && self.frame().is_some_and(|f| f.mouse_tracking)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus(window, cx);
        self.host.call(
            "session.focus_block",
            json!({"session_id": self.session_id, "block_id": self.block_id}),
        );
        let at = self.cell_at(event.position);
        let button = match event.button {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
            _ => return,
        };
        if self.tracking(&event.modifiers) {
            self.reporting = Some(button);
            self.mouse_report("press", Some(button), at, &event.modifiers);
            return;
        }
        let Some(pane) = self.pane() else { return };
        match event.button {
            MouseButton::Left if event.modifiers.shift => {
                pane.send(PaneCommand::SelectTo { x: at.0, y: at.1 });
                self.selecting = true;
            }
            MouseButton::Left => {
                let mode = match (event.click_count, event.modifiers.alt) {
                    (_, true) => SelectMode::Rectangle,
                    (2, _) => SelectMode::Word,
                    (n, _) if n >= 3 => SelectMode::Line,
                    _ => SelectMode::Cell,
                };
                // A plain click anchors a selection that starts once the pointer moves.
                pane.send(PaneCommand::ClearSelection);
                pane.send(PaneCommand::Select {
                    x: at.0,
                    y: at.1,
                    mode,
                });
                self.selecting = true;
            }
            MouseButton::Middle => self.paste(cx),
            _ => {}
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let at = self.cell_at(event.position);
        if let Some(button) = self.reporting {
            self.mouse_report("motion", Some(button), at, &event.modifiers);
            return;
        }
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            if let Some(pane) = self.pane() {
                pane.send(PaneCommand::SelectTo { x: at.0, y: at.1 });
            }
            cx.notify();
        } else if self.tracking(&event.modifiers) && event.pressed_button.is_none() {
            self.mouse_report("motion", None, at, &event.modifiers);
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let at = self.cell_at(event.position);
        if let Some(button) = self.reporting.take() {
            self.mouse_report("release", Some(button), at, &event.modifiers);
            return;
        }
        if self.selecting {
            self.selecting = false;
            if self.copy_on_select {
                self.copy(cx);
            }
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(cell) = self.cell else { return };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / cell.height,
        };
        let whole = lines.round() as i32;
        if whole == 0 {
            return;
        }
        let frame = self.frame();
        let app_scrolls = frame
            .as_ref()
            .is_some_and(|f| (f.mouse_tracking || f.alternate_screen) && !event.modifiers.shift);
        if app_scrolls {
            let at = self.cell_at(event.position);
            // Up is negative for Rex, as for the terminal engine.
            let step = -whole.signum();
            for _ in 0..whole.unsigned_abs().min(8) {
                self.block_event(
                    "wheel",
                    json!({"x": at.0, "y": at.1, "delta_x": 0, "delta_y": step}),
                    false,
                );
            }
        } else {
            self.scroll(Scroll::Lines(-(whole as isize) * 3));
        }
        cx.notify();
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Text attributes that start a new text run when they change.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Hsla,
    bold: bool,
    italic: bool,
    underline: Option<bool>,
    strike: bool,
}

struct Segment {
    col: u16,
    text: String,
    runs: Vec<(usize, Style)>,
    /// Glyphs advance exactly one cell; wide and composed characters paint alone.
    forced: Option<u16>,
}

struct Palette {
    selection_bg: Hsla,
    selection_fg: Hsla,
    match_bg: Hsla,
    current_bg: Hsla,
    current_fg: Hsla,
}

fn segments(
    cells: &[crate::rex::stream::Cell],
    palette: &Palette,
) -> (Vec<Segment>, Vec<(u16, u16, Hsla)>) {
    let mut segments: Vec<Segment> = Vec::new();
    let mut backgrounds: Vec<(u16, u16, Hsla)> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        let col = col as u16;
        let mut fg = color(cell.fg);
        let mut bg = cell.bg.map(color);
        if cell.has(flags::CURRENT_MATCH) {
            bg = Some(palette.current_bg);
            fg = palette.current_fg;
        } else if cell.has(flags::MATCH) {
            bg = Some(palette.match_bg);
        } else if cell.has(flags::SELECTED) {
            bg = Some(palette.selection_bg);
            fg = palette.selection_fg;
        }
        if cell.has(flags::FAINT) {
            fg.a *= 0.6;
        }
        if let Some(bg) = bg {
            match backgrounds.last_mut() {
                Some((_, end, last)) if *end == col && *last == bg => *end = col + 1,
                _ => backgrounds.push((col, col + 1, bg)),
            }
        }
        if cell.has(flags::SPACER) || cell.text.is_empty() || cell.text == " " {
            continue;
        }
        let style = Style {
            fg,
            bold: cell.has(flags::BOLD),
            italic: cell.has(flags::ITALIC),
            underline: (cell.has(flags::UNDERLINE)
                || cell.has(flags::DOUBLE_UNDERLINE)
                || cell.has(flags::CURLY_UNDERLINE)
                || cell.has(flags::LINK))
            .then_some(cell.has(flags::CURLY_UNDERLINE)),
            strike: cell.has(flags::STRIKE),
        };
        let alone = cell.has(flags::WIDE) || cell.text.chars().count() > 1;
        if alone {
            segments.push(Segment {
                col,
                text: cell.text.clone(),
                runs: vec![(cell.text.len(), style)],
                forced: None,
            });
            continue;
        }
        let continues = segments
            .last()
            .is_some_and(|s| s.forced.is_some() && s.col + s.forced.unwrap_or(0) == col);
        if !continues {
            segments.push(Segment {
                col,
                text: String::new(),
                runs: Vec::new(),
                forced: Some(0),
            });
        }
        let segment = segments.last_mut().expect("segment");
        segment.text.push_str(&cell.text);
        segment.forced = segment.forced.map(|n| n + 1);
        match segment.runs.last_mut() {
            Some((len, last)) if *last == style => *len += cell.text.len(),
            _ => segment.runs.push((cell.text.len(), style)),
        }
    }
    (segments, backgrounds)
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let cell = self.measure(window);
        let model = self.pane().map(|p| p.snapshot());
        let frame = model.as_ref().and_then(|m| m.frame.clone());
        let focused = self.focus.is_focused(window);
        let background = frame
            .as_ref()
            .map_or(color(theme.terminal.background), |f| color(f.bg));
        let entity = cx.entity();
        let terminal_font = self.terminal_font();
        let font_size = px(self.font.pixels());
        let padding = self.padding;
        let status = model.as_ref().map(|m| m.status.clone());
        let palette = Palette {
            selection_bg: color(theme.terminal.selection_background),
            selection_fg: color(theme.terminal.selection_foreground),
            match_bg: theme.primary_container,
            current_bg: theme.primary,
            current_fg: theme.on_primary,
        };
        let cursor_color = color(theme.terminal.cursor);
        let cursor_text = color(theme.terminal.cursor_text);

        let grid = canvas(
            {
                let entity = entity.clone();
                move |bounds, _window, cx| {
                    entity.update(cx, |pane, _| {
                        pane.origin = bounds.origin + padding;
                        pane.fit(bounds, cell);
                    });
                }
            },
            move |bounds, (), window, cx| {
                window.paint_quad(fill(bounds, background));
                let Some(frame) = frame else { return };
                let origin = bounds.origin + padding;
                let cell_bounds = |col: u16, row: usize, cols: u16| {
                    Bounds::new(
                        point(
                            origin.x + cell.width * col as f32,
                            origin.y + cell.height * row as f32,
                        ),
                        size(cell.width * cols as f32, cell.height),
                    )
                };
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for (row, cells) in frame.lines.iter().enumerate() {
                        let (segments, backgrounds) = segments(cells, &palette);
                        for (start, end, bg) in backgrounds {
                            window.paint_quad(fill(cell_bounds(start, row, end - start), bg));
                        }
                        for segment in segments {
                            let runs: Vec<TextRun> = segment
                                .runs
                                .iter()
                                .map(|(len, style)| TextRun {
                                    len: *len,
                                    font: Font {
                                        weight: if style.bold {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::NORMAL
                                        },
                                        style: if style.italic {
                                            FontStyle::Italic
                                        } else {
                                            FontStyle::Normal
                                        },
                                        ..terminal_font.clone()
                                    },
                                    color: style.fg,
                                    background_color: None,
                                    underline: style.underline.map(|wavy| UnderlineStyle {
                                        thickness: px(1.),
                                        color: Some(style.fg),
                                        wavy,
                                    }),
                                    strikethrough: style.strike.then_some(StrikethroughStyle {
                                        thickness: px(1.),
                                        color: Some(style.fg),
                                    }),
                                })
                                .collect();
                            let force = segment.forced.map(|_| cell.width);
                            let line = window.text_system().shape_line(
                                segment.text.into(),
                                font_size,
                                &runs,
                                force,
                            );
                            let at = cell_bounds(segment.col, row, 1).origin;
                            let _ = line.paint(at, cell.height, TextAlign::Left, None, window, cx);
                        }
                    }
                    if let Some(cursor) = frame.cursor {
                        let width = if cursor.wide { 2 } else { 1 };
                        let area = cell_bounds(cursor.x, cursor.y as usize, width);
                        let color = cursor
                            .color
                            .map(super::theme::color)
                            .unwrap_or(cursor_color);
                        let shape = if focused {
                            cursor.shape
                        } else {
                            CursorShape::HollowBlock
                        };
                        match shape {
                            CursorShape::Block => {
                                window.paint_quad(fill(area, color));
                                let text = frame.lines[cursor.y as usize]
                                    .get(cursor.x as usize)
                                    .map(|c| c.text.clone())
                                    .unwrap_or_default();
                                if !text.trim().is_empty() {
                                    let run = TextRun {
                                        len: text.len(),
                                        font: terminal_font.clone(),
                                        color: cursor_text,
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    };
                                    let line = window.text_system().shape_line(
                                        text.into(),
                                        font_size,
                                        &[run],
                                        Some(cell.width),
                                    );
                                    let _ = line.paint(
                                        area.origin,
                                        cell.height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                }
                            }
                            CursorShape::HollowBlock => {
                                window.paint_quad(gpui::outline(
                                    area,
                                    color,
                                    gpui::BorderStyle::Solid,
                                ));
                            }
                            CursorShape::Bar => {
                                window.paint_quad(fill(
                                    Bounds::new(area.origin, size(px(2.), cell.height)),
                                    color,
                                ));
                            }
                            CursorShape::Underline => {
                                let y = area.origin.y + cell.height - px(2.);
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(area.origin.x, y),
                                        size(area.size.width, px(2.)),
                                    ),
                                    color,
                                ));
                            }
                        }
                    }
                });
            },
        )
        .size_full();

        let notice = match status {
            Some(PaneStatus::Restoring) | None => Some("Restoring…".to_owned()),
            Some(PaneStatus::Reconnecting(reason)) => Some(format!("Reconnecting · {reason}")),
            Some(PaneStatus::Closed) => Some("This terminal closed on the server".to_owned()),
            Some(PaneStatus::Live) => None,
        };

        div()
            .id(SharedString::from(self.block_id.clone()))
            .track_focus(&self.focus)
            .key_context("Terminal")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(background)
            .cursor(gpui::CursorStyle::IBeam)
            .on_key_down(cx.listener(|pane, event: &KeyDownEvent, _, cx| {
                if pane.send_key(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .child(grid)
            .children(notice.map(|text| {
                div()
                    .absolute()
                    .bottom(px(8.))
                    .right(px(8.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(theme.radius_small())
                    .bg(theme.surface_high)
                    .text_color(theme.text_muted)
                    .text_size(px(12.))
                    .font_family(theme.ui_font.clone())
                    .child(text)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rex::stream::Cell;

    fn cell(text: &str, flags: u16) -> Cell {
        Cell {
            text: text.into(),
            fg: 0xffffff,
            bg: None,
            flags,
        }
    }

    fn palette() -> Palette {
        Palette {
            selection_bg: color(0x333333),
            selection_fg: color(0xeeeeee),
            match_bg: color(0x444444),
            current_bg: color(0x555555),
            current_fg: color(0x000000),
        }
    }

    #[test]
    fn runs_split_at_style_changes_and_wide_cells_paint_alone() {
        let row = vec![
            cell("a", 0),
            cell("b", 0),
            cell("c", flags::BOLD),
            cell("界", flags::WIDE),
            cell("", flags::SPACER),
            cell("d", 0),
            cell(" ", 0),
            cell("e", 0),
        ];
        let (segments, backgrounds) = segments(&row, &palette());
        assert!(backgrounds.is_empty());
        let shape: Vec<_> = segments
            .iter()
            .map(|s| (s.col, s.text.as_str(), s.runs.len(), s.forced))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, "abc", 2, Some(3)),
                (3, "界", 1, None),
                (5, "d", 1, Some(1)),
                (7, "e", 1, Some(1))
            ]
        );
    }

    #[test]
    fn backgrounds_merge_and_highlights_win() {
        let mut row = vec![
            cell("x", 0),
            cell("y", 0),
            cell("z", flags::SELECTED),
            cell("w", flags::CURRENT_MATCH),
        ];
        row[0].bg = Some(0x101010);
        row[1].bg = Some(0x101010);
        let (_, backgrounds) = segments(&row, &palette());
        assert_eq!(backgrounds.len(), 3);
        assert_eq!((backgrounds[0].0, backgrounds[0].1), (0, 2));
        assert_eq!(backgrounds[1].2, color(0x333333));
        assert_eq!(backgrounds[2].2, color(0x555555));
    }
}
