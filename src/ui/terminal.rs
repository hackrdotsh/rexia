//! One terminal pane: draws the stream's latest frame on a cell grid and sends typing,
//! pasting, mouse and size changes to the Rex server. Selection, scrollback and search
//! run locally unless the program asked for mouse reports.
use super::{
    fonts::TerminalFont,
    keys,
    theme::{Theme, color},
};
use crate::rex::images::{Layer, Placement};
use crate::rex::{
    host::{HostCommand, HostHandle, PaneCommand, PaneStatus, Scroll},
    stream::{CursorShape, Frame, SelectMode, flags},
};
use crate::settings::{PaddingFill, SelectionColor};
use gpui::RenderImage;
use gpui::{
    App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla,
    KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollEvent, SharedString, StrikethroughStyle, TextAlign, TextRun,
    UnderlineStyle, Window, canvas, div, fill, font, point, prelude::*, px, size,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

/// Links a click opens without asking: web, mail and local files. Other schemes could
/// start programs, so the workspace decides about them.
pub fn openable(link: &str) -> bool {
    crate::ui::workspace::links::ordinary(link)
}

/// What a pane asks of the workspace around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneEvent {
    /// A program's link was ctrl+clicked.
    OpenLink(String),
}

impl gpui::EventEmitter<PaneEvent> for TerminalPane {}

/// The background of the cell at `row`, `col`.
fn cell_bg(frame: &Frame, row: usize, col: usize) -> u32 {
    frame
        .lines
        .get(row)
        .and_then(|cells| cells.get(col))
        .and_then(|cell| cell.bg)
        .unwrap_or(frame.bg)
}

/// One color for the whole edge, if its cells share one.
fn uniform(colors: impl IntoIterator<Item = u32>) -> Option<u32> {
    let mut colors = colors.into_iter();
    let first = colors.next()?;
    colors.all(|c| c == first).then_some(first)
}

/// The colors the top edge gives the padding and the pane header, if any.
fn top_fill(frame: &Frame, mode: PaddingFill) -> Option<u32> {
    let active = match mode {
        PaddingFill::Off => false,
        PaddingFill::Automatic => frame.alternate_screen,
        PaddingFill::Always => true,
    };
    active
        .then(|| uniform((0..frame.cols as usize).map(|c| cell_bg(frame, 0, c))))
        .flatten()
}

/// Areas of the padding around the grid and the program colors that fill them: each edge
/// cell's background reaches out to the pane's edge, as Ghostty's `window-padding-color =
/// extend` does. Automatic fills only while a full-screen program runs, and only along
/// edges of one color.
fn padding_fills(
    frame: &Frame,
    mode: PaddingFill,
    bounds: Bounds<Pixels>,
    origin: Point<Pixels>,
    cell: CellSize,
) -> Vec<(Bounds<Pixels>, u32)> {
    let automatic = match mode {
        PaddingFill::Off => return Vec::new(),
        PaddingFill::Automatic if !frame.alternate_screen => return Vec::new(),
        PaddingFill::Automatic => true,
        PaddingFill::Always => false,
    };
    let (cols, rows) = (
        frame.cols as usize,
        frame.lines.len().min(frame.rows as usize),
    );
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let left = bounds.origin.x;
    let top = bounds.origin.y;
    let right = bounds.origin.x + bounds.size.width;
    let bottom = bounds.origin.y + bounds.size.height;
    let grid_right = origin.x + cell.width * cols as f32;
    let grid_bottom = origin.y + cell.height * rows as f32;
    let x = |c: usize| origin.x + cell.width * c as f32;
    let y = |r: usize| origin.y + cell.height * r as f32;
    let rect = |x0: Pixels, y0: Pixels, x1: Pixels, y1: Pixels| {
        Bounds::new(
            point(x0, y0),
            size((x1 - x0).max(px(0.)), (y1 - y0).max(px(0.))),
        )
    };
    // An Automatic edge fills only if its cells share one color.
    let edge = |colors: Vec<u32>| -> Option<Vec<u32>> {
        (!automatic || uniform(colors.iter().copied()).is_some()).then_some(colors)
    };
    let mut fills = Vec::new();
    if let Some(colors) = edge((0..cols).map(|c| cell_bg(frame, 0, c)).collect()) {
        for (c, rgb) in colors.into_iter().enumerate() {
            fills.push((rect(x(c), top, x(c + 1), origin.y), rgb));
        }
    }
    if let Some(colors) = edge((0..cols).map(|c| cell_bg(frame, rows - 1, c)).collect()) {
        for (c, rgb) in colors.into_iter().enumerate() {
            fills.push((rect(x(c), grid_bottom, x(c + 1), bottom), rgb));
        }
    }
    // The side strips take the corners, reaching up and down from the first and last rows.
    let span = |r: usize| {
        let y0 = if r == 0 { top } else { y(r) };
        let y1 = if r + 1 == rows { bottom } else { y(r + 1) };
        (y0, y1)
    };
    if let Some(colors) = edge((0..rows).map(|r| cell_bg(frame, r, 0)).collect()) {
        for (r, rgb) in colors.into_iter().enumerate() {
            let (y0, y1) = span(r);
            fills.push((rect(left, y0, origin.x, y1), rgb));
        }
    }
    if let Some(colors) = edge((0..rows).map(|r| cell_bg(frame, r, cols - 1)).collect()) {
        for (r, rgb) in colors.into_iter().enumerate() {
            let (y0, y1) = span(r);
            fills.push((rect(grid_right, y0, right, y1), rgb));
        }
    }
    fills.retain(|(area, _)| area.size.width > px(0.) && area.size.height > px(0.));
    fills
}

/// One image's part, as GPUI draws it: the source rectangle cut out, in BGRA.
fn texture(placement: &Placement) -> Option<Arc<RenderImage>> {
    let pixels = &placement.pixels;
    let [x, y, w, h] = placement.source;
    let (x, y) = (x.min(pixels.width), y.min(pixels.height));
    let (w, h) = (w.min(pixels.width - x), h.min(pixels.height - y));
    if w == 0 || h == 0 {
        return None;
    }
    let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y..y + h {
        let start = (row as usize * pixels.width as usize + x as usize) * 4;
        let line = pixels.rgba.get(start..start + w as usize * 4)?;
        for p in line.as_chunks::<4>().0 {
            bgra.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    let buffer = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Where an image draws: its cell, offset and size are in the terminal's pixels, which
/// scale to this pane's cells.
fn image_area(
    placement: &Placement,
    cell_px: (u32, u32),
    origin: Point<Pixels>,
    cell: CellSize,
) -> Bounds<Pixels> {
    let scale = |ours: Pixels, theirs: u32| {
        if theirs == 0 {
            1.
        } else {
            f32::from(ours) / theirs as f32
        }
    };
    let (sx, sy) = (scale(cell.width, cell_px.0), scale(cell.height, cell_px.1));
    Bounds::new(
        point(
            origin.x + cell.width * placement.col as f32 + px(placement.offset_x as f32 * sx),
            origin.y + cell.height * placement.row as f32 + px(placement.offset_y as f32 * sy),
        ),
        size(
            px(placement.width as f32 * sx),
            px(placement.height as f32 * sy),
        ),
    )
}

/// How long the copy shine takes to cross the selection.
const SHINE: std::time::Duration = std::time::Duration::from_millis(450);

/// The selection's leftmost and rightmost columns on screen, which the shine crosses.
fn selection_columns(frame: &Frame) -> Option<(u16, u16)> {
    let mut span: Option<(u16, u16)> = None;
    for cells in &frame.lines {
        for (col, cell) in cells.iter().enumerate() {
            if cell.has(flags::SELECTED) {
                let col = col as u16;
                span = Some(span.map_or((col, col + 1), |(a, b)| (a.min(col), b.max(col + 1))));
            }
        }
    }
    span
}

/// Whether the cell at `row`, `col` is selected.
fn selected_at(frame: &Frame, row: isize, col: u16) -> bool {
    usize::try_from(row)
        .ok()
        .and_then(|r| frame.lines.get(r))
        .and_then(|cells| cells.get(usize::from(col)))
        .is_some_and(|cell| cell.has(flags::SELECTED))
}

/// Rounded corners for a selected run: only the selection's outer corners round, so
/// rows that touch stay joined.
fn selection_corners(
    frame: &Frame,
    row: usize,
    start: u16,
    end: u16,
    cell: CellSize,
) -> gpui::Corners<Pixels> {
    let radius = (cell.height * 0.3).min(px(6.));
    let row = row as isize;
    let last = end.saturating_sub(1);
    let open = |r: isize, c: u16| {
        if selected_at(frame, r, c) {
            px(0.)
        } else {
            radius
        }
    };
    gpui::Corners {
        top_left: open(row - 1, start),
        top_right: open(row - 1, last),
        bottom_left: open(row + 1, start),
        bottom_right: open(row + 1, last),
    }
}

/// The copy shine over one selected run: a light band sweeping left to right across the
/// whole selection at progress `t`, clipped to the run. `grid_left` is the grid's left edge.
fn paint_shine(
    window: &mut Window,
    area: Bounds<Pixels>,
    grid_left: Pixels,
    span: Option<(u16, u16)>,
    t: f32,
    cell: CellSize,
) {
    let Some((first, last)) = span else { return };
    let band = cell.width * 6.;
    let start = grid_left + cell.width * f32::from(first) - band;
    let travel = cell.width * f32::from(last - first) + band * 2.;
    let center = start + travel * t;
    let glow = gpui::hsla(0., 0., 1., 0.4 * (1. - t * 0.5));
    let clear = gpui::hsla(0., 0., 1., 0.);
    let half = band / 2.;
    window.with_content_mask(Some(gpui::ContentMask { bounds: area }), |window| {
        for (from, a, b) in [(center - half, clear, glow), (center, glow, clear)] {
            let piece = Bounds::new(point(from, area.origin.y), size(half, area.size.height));
            window.paint_quad(fill(
                piece,
                gpui::linear_gradient(
                    90.,
                    gpui::linear_color_stop(a, 0.),
                    gpui::linear_color_stop(b, 1.),
                ),
            ));
        }
    });
}

/// Measured once per font: the size of one cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSize {
    pub width: Pixels,
    pub height: Pixels,
}

pub struct TerminalPane {
    /// Kitty graphics ready to draw, by image, generation and the part shown.
    textures: HashMap<(u32, u64, [u32; 4]), Arc<RenderImage>>,
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
    /// Corrections to program colors.
    pub colors: TextColors,
    /// Whether a program's colors fill the padding.
    pub padding_fill: PaddingFill,
    /// How selections look.
    pub selection: crate::settings::SelectionLook,
    /// When the copy shine started, and how many copies have shone.
    shine: Option<std::time::Instant>,
    /// Move history by pixels on touchpads, and the part of a line scrolled so far.
    pub smooth_scrolling: bool,
    scroll_carry: f32,
    /// While panes move, the terminal keeps its size until then.
    size_hold: Option<std::time::Instant>,
    shines: usize,
    drawn_selection: Option<(Option<Hsla>, Hsla)>,
    /// The trail effect drawn when the cursor jumps; none while cursor shaders are off.
    pub cursor_shader: Option<crate::settings::CursorShader>,
    /// Where the focused cursor was last drawn, and the trail it's drawing now.
    last_cursor: Option<crate::ui::cursor_trail::CellSpot>,
    trail: Option<crate::ui::cursor_trail::Trail>,
    /// The pane model revision last drawn.
    pub revision: u64,
    /// A shape to draw whatever the program asks for.
    pub force_cursor: Option<CursorShape>,
    /// The time between blinks for cursors that ask to blink; none never blinks.
    pub blink: Option<std::time::Duration>,
    /// The blinking cursor's current phase.
    blink_on: bool,
    /// Whether the last paint drew a blinking cursor, which the blink timer then toggles.
    blinking: bool,
    /// Typing keeps the cursor solid until this moment.
    hold_until: std::time::Instant,
    /// The cursor shape last drawn, for tests and diagnostics.
    drawn_cursor: Option<CursorShape>,
    /// Text an input method is composing, shown at the cursor until it's committed.
    preedit: Option<String>,
    _blink_timer: gpui::Task<()>,
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
            colors: TextColors::default(),
            padding_fill: PaddingFill::Off,
            selection: Default::default(),
            shine: None,
            size_hold: None,
            smooth_scrolling: true,
            scroll_carry: 0.,
            shines: 0,
            drawn_selection: None,
            cursor_shader: None,
            last_cursor: None,
            trail: None,
            textures: HashMap::new(),
            revision: 0,
            force_cursor: None,
            blink: None,
            blink_on: true,
            blinking: false,
            hold_until: std::time::Instant::now(),
            drawn_cursor: None,
            preedit: None,
            _blink_timer: cx.spawn(async move |this, cx| {
                loop {
                    let Ok(interval) = this.read_with(cx, |pane, _| {
                        pane.blink.unwrap_or(std::time::Duration::from_millis(500))
                    }) else {
                        break;
                    };
                    cx.background_executor().timer(interval).await;
                    if this.update(cx, |pane, cx| pane.blink_tick(cx)).is_err() {
                        break;
                    }
                }
            }),
        }
    }

    /// Advance the blink: a blinking cursor shows or hides; any other stays shown.
    pub fn blink_tick(&mut self, cx: &mut Context<Self>) {
        let next = if self.blinking && self.blink.is_some() {
            std::time::Instant::now() < self.hold_until || !self.blink_on
        } else {
            true
        };
        if next != self.blink_on {
            self.blink_on = next;
            cx.notify();
        }
    }

    /// The colors a program's edges give the padding now, for tests and diagnostics.
    pub fn padding_fill_colors(&self) -> Vec<u32> {
        let (Some(frame), Some(cell)) = (self.frame(), self.cell) else {
            return Vec::new();
        };
        let bounds = Bounds::new(Point::default(), size(px(2000.), px(2000.)));
        let mut colors: Vec<u32> =
            padding_fills(&frame, self.padding_fill, bounds, self.padding, cell)
                .into_iter()
                .map(|(_, rgb)| rgb)
                .collect();
        colors.sort_unstable();
        colors.dedup();
        colors
    }

    /// The color the pane header takes to match the program, while the padding fills.
    pub fn header_fill(&self) -> Option<u32> {
        top_fill(&*self.frame()?, self.padding_fill)
    }

    /// The frame's images with their textures, made once per image and part. Textures
    /// no longer on screen leave the GPU.
    fn image_textures(
        &mut self,
        frame: Option<&Frame>,
        window: &mut Window,
    ) -> Vec<(Placement, Arc<RenderImage>)> {
        let placements = frame.map(|f| f.images.as_slice()).unwrap_or_default();
        let mut live = HashMap::with_capacity(placements.len());
        let mut drawn = Vec::with_capacity(placements.len());
        for placement in placements {
            let key = (
                placement.pixels.id,
                placement.pixels.generation,
                placement.source,
            );
            let texture = match self
                .textures
                .remove(&key)
                .or_else(|| live.get(&key).cloned())
            {
                Some(texture) => texture,
                None => match texture(placement) {
                    Some(texture) => texture,
                    None => continue,
                },
            };
            live.insert(key, texture.clone());
            drawn.push((placement.clone(), texture));
        }
        for (_, stale) in std::mem::replace(&mut self.textures, live) {
            let _ = window.drop_image(stale);
        }
        drawn
    }

    /// The images drawn now, by image ID, for tests and diagnostics: where each is
    /// relative to the grid, in this pane's pixels.
    pub fn image_areas(&self) -> Vec<(u32, Bounds<Pixels>)> {
        let (Some(frame), Some(cell)) = (self.frame(), self.cell) else {
            return Vec::new();
        };
        frame
            .images
            .iter()
            .map(|p| {
                (
                    p.pixels.id,
                    image_area(p, frame.cell_px, Point::default(), cell),
                )
            })
            .collect()
    }

    /// Start a trail when the focused cursor jumps, and the trail to draw now, with its
    /// progress. Unfocused panes and panes without a shader draw none.
    fn track_trail(
        &mut self,
        cursor: Option<crate::rex::stream::Cursor>,
    ) -> Option<(crate::ui::cursor_trail::Trail, f32)> {
        use crate::ui::cursor_trail::{CellSpot, MIN_JUMP, Trail};
        let (Some(shader), Some(cursor)) = (self.cursor_shader, cursor) else {
            self.last_cursor = None;
            self.trail = None;
            return None;
        };
        let spot = CellSpot {
            col: cursor.x,
            row: cursor.y,
            width: if cursor.wide { 2 } else { 1 },
        };
        if let Some(last) = self.last_cursor
            && last != spot
            && last.distance(spot) >= MIN_JUMP
        {
            self.trail = Some(Trail {
                from: last,
                to: spot,
                started: std::time::Instant::now(),
                shader,
            });
        }
        self.last_cursor = Some(spot);
        let trail = self.trail?;
        match trail.progress(std::time::Instant::now()) {
            Some(t) => Some((trail, t)),
            None => {
                self.trail = None;
                None
            }
        }
    }

    /// How far the text is drawn below (or above) its lines while smooth scrolling, in
    /// pixels, for tests.
    pub fn scroll_offset(&self) -> Pixels {
        self.cell.map_or(px(0.), |c| c.height * self.scroll_carry)
    }

    /// The trail the cursor is drawing, for tests: where it jumped from and to.
    pub fn cursor_trail(&self) -> Option<crate::ui::cursor_trail::Trail> {
        self.trail
    }

    /// The size of one character cell, once measured.
    pub fn cell_size(&self) -> Option<CellSize> {
        self.cell
    }

    /// The middle of the cell at `col`, `row`, in window coordinates, for tests.
    pub fn cell_point(&self, col: u16, row: u16) -> Option<Point<Pixels>> {
        let cell = self.cell?;
        Some(
            self.origin
                + point(
                    cell.width * (f32::from(col) + 0.5),
                    cell.height * (f32::from(row) + 0.5),
                ),
        )
    }

    /// The selection's text and fill colors as last drawn (`None` text keeps the
    /// program's colors), for tests.
    pub fn selection_colors(&self) -> Option<(Option<Hsla>, Hsla)> {
        self.drawn_selection
    }

    /// The color a cell's text draws in, after the color settings.
    pub fn drawn_text_color(&self, row: usize, col: usize) -> Option<u32> {
        let frame = self.frame()?;
        let cell = frame.lines.get(row)?.get(col)?;
        let look = Look {
            palette: Palette {
                selection_bg: Hsla::default(),
                selection_fg: None,
                match_bg: Hsla::default(),
                current_bg: Hsla::default(),
                current_fg: Hsla::default(),
            },
            bold: self.font.bold,
            italic: self.font.italic,
            colors: self.colors,
            default_fg: frame.fg,
            default_bg: frame.bg,
            ansi: frame.ansi,
        };
        Some(look.text_color(cell.fg, cell.bg))
    }

    /// Text an input method is composing here.
    pub fn preedit(&self) -> Option<&str> {
        self.preedit.as_deref()
    }

    /// Type text an input method committed, or a character no key event names.
    fn commit_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.preedit = None;
        if !text.is_empty() {
            self.back_to_live();
            self.blink_on = true;
            self.input(
                "client.key",
                json!({"session_id": self.session_id, "block_id": self.block_id,
                    "key": {"key": "unidentified", "mods": "", "text": text,
                        "unshifted_codepoint": 0, "action": "press"}}),
            );
        }
        cx.notify();
    }

    /// The cursor cell in window coordinates, where input methods put their candidates.
    fn cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let cell = self.cell?;
        let cursor = self.frame()?.cursor?;
        Some(Bounds::new(
            point(
                self.origin.x + cell.width * cursor.x as f32,
                self.origin.y + cell.height * cursor.y as f32,
            ),
            size(cell.width, cell.height),
        ))
    }

    /// The cursor as last drawn: its shape, whether it blinks, and whether it's showing.
    pub fn cursor_state(&self) -> Option<(CursorShape, bool, bool)> {
        self.drawn_cursor
            .map(|shape| (shape, self.blinking, self.blink_on || !self.blinking))
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
        if !self.font.ligatures {
            font.features = gpui::FontFeatures::disable_ligatures();
        }
        if !self.font.fallbacks.is_empty() {
            font.fallbacks = Some(gpui::FontFallbacks::from_fonts(self.font.fallbacks.clone()));
        }
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
            width: (width + px(self.font.spacing)).max(px(1.)),
            height,
        };
        self.cell = Some(cell);
        cell
    }

    /// Keep the terminal's size until `until`, while panes move; it fits afterwards.
    pub fn hold_size_until(&mut self, until: std::time::Instant) {
        self.size_hold = Some(until);
    }

    /// Ask the server for the grid that fits `bounds`.
    fn fit(&mut self, bounds: Bounds<Pixels>, cell: CellSize) {
        if self
            .size_hold
            .is_some_and(|t| std::time::Instant::now() < t)
            && self.requested.is_some()
        {
            return;
        }
        self.size_hold = None;
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
        // The cursor stays solid while typing.
        self.blink_on = true;
        self.hold_until = std::time::Instant::now() + self.blink.unwrap_or_default();
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
        cx.spawn(async move |this, cx| {
            if let Some(Some(text)) = pane.ask(PaneCommand::Copy).await
                && !text.is_empty()
            {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
                // A shine across the selection confirms the copy.
                let _ = this.update(cx, |pane, cx| {
                    if pane.selection.copy_animation {
                        pane.shine = Some(std::time::Instant::now());
                        pane.shines += 1;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// How far the copy shine has gone, from 0 to 1, while it runs.
    fn shine_progress(&mut self) -> Option<f32> {
        let t = self.shine?.elapsed().as_secs_f32() / SHINE.as_secs_f32();
        if t >= 1. {
            self.shine = None;
            return None;
        }
        Some(t)
    }

    /// How many copies have shone, for tests.
    pub fn copy_shines(&self) -> usize {
        self.shines
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
        // Ctrl-click opens a program's OSC 8 link, as in Ghostty.
        if event.button == MouseButton::Left
            && event.modifiers.control
            && !event.modifiers.shift
            && let Some(pane) = self.pane()
        {
            let (x, y) = at;
            cx.spawn(async move |this, cx| {
                let link = pane
                    .ask(|reply| PaneCommand::LinkAt { x, y, reply })
                    .await
                    .flatten();
                if let Some(link) = link {
                    let _ = this.update(cx, |_, cx| cx.emit(PaneEvent::OpenLink(link)));
                }
            })
            .detach();
            return;
        }
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
        let frame = self.frame();
        let app_scrolls = frame
            .as_ref()
            .is_some_and(|f| (f.mouse_tracking || f.alternate_screen) && !event.modifiers.shift);
        // Smooth scrolling: a touchpad moves history by pixels, whole lines through the
        // terminal and the rest by drawing the text that much lower or higher.
        if let (ScrollDelta::Pixels(delta), true, false) =
            (event.delta, self.smooth_scrolling, app_scrolls)
        {
            let bar = frame.as_ref().map(|f| f.scrollbar).unwrap_or_default();
            self.scroll_carry += delta.y / cell.height;
            let whole = self.scroll_carry.trunc();
            self.scroll_carry -= whole;
            if whole != 0. {
                self.scroll(Scroll::Lines(-(whole as isize)));
            }
            // History has an end at each side: the top of the scrollback, and live output.
            let at_top = bar.offset == 0 && whole >= 0.;
            let at_bottom = bar.offset + bar.visible >= bar.total && whole <= 0.;
            if (self.scroll_carry > 0. && at_top) || (self.scroll_carry < 0. && at_bottom) {
                self.scroll_carry = 0.;
            }
            cx.notify();
            return;
        }
        self.scroll_carry = 0.;
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / cell.height,
        };
        let whole = lines.round() as i32;
        if whole == 0 {
            return;
        }
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

#[derive(Clone, Copy)]
struct Palette {
    selection_bg: Hsla,
    /// The text color inside a selection; `None` keeps the program's colors.
    selection_fg: Option<Hsla>,
    match_bg: Hsla,
    current_bg: Hsla,
    current_fg: Hsla,
}

/// How program colors are corrected before they're drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextColors {
    pub improve_contrast: bool,
    pub theme_palette: bool,
}

/// What a frame's text draws with: its styles, and colors corrected as chosen.
struct Look {
    palette: Palette,
    bold: bool,
    italic: bool,
    colors: TextColors,
    default_fg: u32,
    default_bg: u32,
    ansi: [u32; 16],
}

impl Look {
    /// A cell's text color, mapped to the theme's palette and lifted off its background
    /// as the settings ask.
    fn text_color(&self, fg: u32, bg: Option<u32>) -> u32 {
        let mut fg = fg;
        if self.colors.theme_palette {
            let mut themed = self.ansi.to_vec();
            themed.push(self.default_fg);
            fg = super::text_color::nearest(fg, &themed);
        }
        if self.colors.improve_contrast || self.colors.theme_palette {
            fg = super::text_color::ensure_contrast(
                fg,
                bg.unwrap_or(self.default_bg),
                super::text_color::MIN_CONTRAST,
            );
        }
        fg
    }
}

/// A run of cells sharing a background: first column, end column, color, and whether
/// it's part of a selection.
type Background = (u16, u16, Hsla, bool);

fn segments(cells: &[crate::rex::stream::Cell], look: &Look) -> (Vec<Segment>, Vec<Background>) {
    let palette = &look.palette;
    let mut segments: Vec<Segment> = Vec::new();
    let mut backgrounds: Vec<Background> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        let col = col as u16;
        let mut fg = color(look.text_color(cell.fg, cell.bg));
        let mut bg = cell.bg.map(color);
        let mut selected = false;
        if cell.has(flags::CURRENT_MATCH) {
            bg = Some(palette.current_bg);
            fg = palette.current_fg;
        } else if cell.has(flags::MATCH) {
            bg = Some(palette.match_bg);
        } else if cell.has(flags::SELECTED) {
            bg = Some(palette.selection_bg);
            fg = palette.selection_fg.unwrap_or(fg);
            selected = true;
        }
        if cell.has(flags::FAINT) {
            fg.a *= 0.6;
        }
        if let Some(bg) = bg {
            match backgrounds.last_mut() {
                Some((_, end, last, was)) if *end == col && *last == bg && *was == selected => {
                    *end = col + 1
                }
                _ => backgrounds.push((col, col + 1, bg, selected)),
            }
        }
        if cell.has(flags::SPACER) || cell.text.is_empty() || cell.text == " " {
            continue;
        }
        let style = Style {
            fg,
            bold: look.bold && cell.has(flags::BOLD),
            italic: look.italic && cell.has(flags::ITALIC),
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
        let selection = self.selection;
        let ansi = frame.as_ref().map(|f| f.ansi);
        let pick = |choice: SelectionColor, theme_color: u32| match choice {
            SelectionColor::Theme => Some(color(theme_color)),
            SelectionColor::Accent => Some(theme.primary),
            SelectionColor::Keep => None,
            SelectionColor::Palette(index) => Some(color(
                ansi.map_or(theme.terminal.ansi[usize::from(index % 16)], |a| {
                    a[usize::from(index % 16)]
                }),
            )),
        };
        let shine = self.shine_progress();
        if shine.is_some() {
            window.request_animation_frame();
        }
        let selection_span = frame.as_deref().and_then(selection_columns);
        let palette = Palette {
            selection_bg: pick(selection.background, theme.terminal.selection_background)
                .unwrap_or(color(theme.terminal.selection_background)),
            selection_fg: pick(selection.foreground, theme.terminal.selection_foreground),
            match_bg: theme.primary_container,
            current_bg: theme.primary,
            current_fg: theme.on_primary,
        };
        self.drawn_selection = Some((palette.selection_fg, palette.selection_bg));
        let (bold, italic, colors) = (self.font.bold, self.font.italic, self.colors);
        let padding_fill = self.padding_fill;
        let scroll_shift = self.scroll_offset();
        let cursor_color = color(theme.terminal.cursor);
        let cursor_text = color(theme.terminal.cursor_text);
        // Only a focused pane's cursor blinks, and only if its program asks.
        let cursor = frame.as_ref().and_then(|f| f.cursor);
        self.blinking = focused && self.blink.is_some() && cursor.is_some_and(|c| c.blinking);
        let cursor_hidden = self.blinking && !self.blink_on;
        let cursor_shape = cursor.map(|c| {
            if focused {
                self.force_cursor.unwrap_or(c.shape)
            } else {
                CursorShape::HollowBlock
            }
        });
        self.drawn_cursor = cursor_shape;
        let trail = self.track_trail(cursor.filter(|_| focused));
        if trail.is_some() {
            window.request_animation_frame();
        }
        let accent = theme.primary;
        let preedit = self.preedit.clone().filter(|p| !p.is_empty() && focused);
        let focus_handle = self.focus.clone();
        let images = self.image_textures(frame.as_deref(), window);

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
                window.handle_input(
                    &focus_handle,
                    gpui::ElementInputHandler::new(bounds, entity.clone()),
                    cx,
                );
                window.paint_quad(fill(bounds, background));
                let Some(frame) = frame else { return };
                let origin = bounds.origin + padding + point(px(0.), scroll_shift);
                for (area, rgb) in padding_fills(&frame, padding_fill, bounds, origin, cell) {
                    window.paint_quad(fill(area, color(rgb)));
                }
                // Kitty graphics draw in three layers around the cell backgrounds and text.
                let paint_images = |layer: Layer, window: &mut Window| {
                    for (placement, texture) in images.iter().filter(|(p, _)| p.layer == layer) {
                        let area = image_area(placement, frame.cell_px, origin, cell);
                        let _ = window.paint_image(
                            bounds,
                            area,
                            gpui::Corners::default(),
                            texture.clone(),
                            0,
                            false,
                        );
                    }
                };
                let cell_bounds = |col: u16, row: usize, cols: u16| {
                    Bounds::new(
                        point(
                            origin.x + cell.width * col as f32,
                            origin.y + cell.height * row as f32,
                        ),
                        size(cell.width * cols as f32, cell.height),
                    )
                };
                let look = Look {
                    palette,
                    bold,
                    italic,
                    colors,
                    default_fg: frame.fg,
                    default_bg: frame.bg,
                    ansi: frame.ansi,
                };
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    paint_images(Layer::BelowBackground, window);
                    let mut rows = Vec::with_capacity(frame.lines.len());
                    for (row, cells) in frame.lines.iter().enumerate() {
                        let (segments, backgrounds) = segments(cells, &look);
                        for (start, end, bg, selected) in backgrounds {
                            let area = cell_bounds(start, row, end - start);
                            if selected && selection.rounded {
                                let corners = selection_corners(&frame, row, start, end, cell);
                                window.paint_quad(fill(area, bg).corner_radii(corners));
                            } else {
                                window.paint_quad(fill(area, bg));
                            }
                            if selected && let Some(t) = shine {
                                paint_shine(window, area, origin.x, selection_span, t, cell);
                            }
                        }
                        rows.push(segments);
                    }
                    paint_images(Layer::BelowText, window);
                    for (row, segments) in rows.into_iter().enumerate() {
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
                    paint_images(Layer::AboveText, window);
                    // A cursor shader's trail, under the cursor it leads to.
                    if let Some((trail, t)) = &trail {
                        let color = frame
                            .cursor
                            .and_then(|c| c.color)
                            .map(super::theme::color)
                            .unwrap_or(cursor_color);
                        crate::ui::cursor_trail::paint_trail(
                            window,
                            trail,
                            *t,
                            |spot| cell_bounds(spot.col, spot.row as usize, spot.width),
                            color,
                            accent,
                        );
                    }
                    if let (Some(cursor), Some(shape), false) =
                        (frame.cursor, cursor_shape, cursor_hidden)
                    {
                        let width = if cursor.wide { 2 } else { 1 };
                        let area = cell_bounds(cursor.x, cursor.y as usize, width);
                        let color = cursor
                            .color
                            .map(super::theme::color)
                            .unwrap_or(cursor_color);
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
                    // What an input method is composing covers the cells from the cursor on,
                    // a wide character taking two, as the terminal will lay it out.
                    if let (Some(text), Some(cursor)) = (preedit.clone(), frame.cursor) {
                        use unicode_width::UnicodeWidthChar;
                        let fg = super::theme::color(frame.fg);
                        let mut col = cursor.x;
                        for c in text.chars() {
                            let cells = c.width().unwrap_or(0).max(1) as u16;
                            let area = cell_bounds(col, cursor.y as usize, cells);
                            window.paint_quad(fill(area, background));
                            let glyph = c.to_string();
                            let run = TextRun {
                                len: glyph.len(),
                                font: terminal_font.clone(),
                                color: fg,
                                background_color: None,
                                underline: Some(UnderlineStyle {
                                    thickness: px(1.),
                                    color: Some(fg),
                                    wavy: false,
                                }),
                                strikethrough: None,
                            };
                            let line = window.text_system().shape_line(
                                glyph.into(),
                                font_size,
                                &[run],
                                Some(area.size.width),
                            );
                            let _ = line.paint(
                                area.origin,
                                cell.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            col += cells;
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

impl gpui::EntityInputHandler for TerminalPane {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text: Vec<u16> = self
            .preedit
            .as_deref()
            .unwrap_or("")
            .encode_utf16()
            .collect();
        let range = range.start.min(text.len())..range.end.min(text.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&text[range]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        let end = self
            .preedit
            .as_deref()
            .map_or(0, |p| p.encode_utf16().count());
        Some(gpui::UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.preedit
            .as_deref()
            .filter(|p| !p.is_empty())
            .map(|p| 0..p.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_text(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit = (!text.is_empty()).then(|| text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds()
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    fn frame_with(bgs: &[&[Option<u32>]], alternate: bool) -> Frame {
        let lines: Vec<Vec<crate::rex::stream::Cell>> = bgs
            .iter()
            .map(|row| {
                row.iter()
                    .map(|bg| crate::rex::stream::Cell {
                        text: " ".into(),
                        fg: 0xffffff,
                        bg: *bg,
                        flags: 0,
                    })
                    .collect()
            })
            .collect();
        Frame {
            cols: bgs[0].len() as u16,
            rows: bgs.len() as u16,
            lines,
            fg: 0xffffff,
            bg: 0x000000,
            cursor: None,
            mouse_tracking: false,
            alternate_screen: alternate,
            scrollbar: Default::default(),
            has_selection: false,
            search: None,
            ansi: [0; 16],
            images: Vec::new(),
            cell_px: (0, 0),
            mediums: Default::default(),
        }
    }

    #[test]
    fn selections_round_only_their_outer_corners() {
        // Two selected rows: columns 2 to 5, then 0 to 3.
        let mut frame = frame_with(&[&[None; 6], &[None; 6]], false);
        for col in 2..6 {
            frame.lines[0][col].flags = flags::SELECTED;
        }
        for col in 0..4 {
            frame.lines[1][col].flags = flags::SELECTED;
        }
        let cell = CellSize {
            width: px(10.),
            height: px(20.),
        };
        assert_eq!(selection_columns(&frame), Some((0, 6)));
        let top = selection_corners(&frame, 0, 2, 6, cell);
        // The first row's bottom-left sits over selected text, so it stays square.
        assert_eq!(
            (
                top.top_left,
                top.top_right,
                top.bottom_left,
                top.bottom_right
            ),
            (px(6.), px(6.), px(0.), px(6.))
        );
        let bottom = selection_corners(&frame, 1, 0, 4, cell);
        assert_eq!(
            (
                bottom.top_left,
                bottom.top_right,
                bottom.bottom_left,
                bottom.bottom_right
            ),
            (px(6.), px(0.), px(6.), px(6.))
        );
    }

    #[test]
    fn padding_takes_edge_colors_as_chosen() {
        let blue = Some(0x2040c0);
        let red = Some(0xc02020);
        let cell = CellSize {
            width: px(10.),
            height: px(20.),
        };
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(60.), px(60.)));
        let origin = point(px(5.), px(5.));
        let colors = |frame: &Frame, mode| {
            let mut c: Vec<u32> = padding_fills(frame, mode, bounds, origin, cell)
                .into_iter()
                .map(|(_, rgb)| rgb)
                .collect();
            c.sort_unstable();
            c.dedup();
            c
        };
        // A full-screen program painted blue: every edge is one color.
        let editor = frame_with(&[&[blue, blue], &[blue, blue]], true);
        assert!(colors(&editor, PaddingFill::Off).is_empty());
        assert_eq!(colors(&editor, PaddingFill::Automatic), vec![0x2040c0]);
        assert_eq!(top_fill(&editor, PaddingFill::Automatic), Some(0x2040c0));
        // A shell: Automatic leaves it alone; Always extends each edge cell.
        let shell = frame_with(&[&[red, None], &[blue, None]], false);
        assert!(colors(&shell, PaddingFill::Automatic).is_empty());
        assert_eq!(
            colors(&shell, PaddingFill::Always),
            vec![0x000000, 0x2040c0, 0xc02020]
        );
        // Mixed edges don't fill in Automatic, even full screen.
        let mixed = frame_with(&[&[red, blue], &[blue, blue]], true);
        assert_eq!(colors(&mixed, PaddingFill::Automatic), vec![0x2040c0]);
        assert_eq!(top_fill(&mixed, PaddingFill::Automatic), None);
    }

    #[test]
    fn only_safe_links_open() {
        assert!(super::openable("https://superlogical.com/rex"));
        assert!(super::openable("file:///home/me/notes.md"));
        assert!(super::openable("mailto:me@example.com"));
        assert!(!super::openable("javascript:alert(1)"));
        assert!(!super::openable("ssh://box"));
        assert!(!super::openable("not a url"));
    }

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
            selection_fg: Some(color(0xeeeeee)),
            match_bg: color(0x444444),
            current_bg: color(0x555555),
            current_fg: color(0x000000),
        }
    }

    fn look() -> Look {
        Look {
            palette: palette(),
            bold: true,
            italic: true,
            colors: TextColors::default(),
            default_fg: 0xeeeeee,
            default_bg: 0x000000,
            ansi: [0; 16],
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
        let (segments, backgrounds) = segments(&row, &look());
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
        let (_, backgrounds) = segments(&row, &look());
        assert_eq!(backgrounds.len(), 3);
        assert!(!backgrounds[0].3 && backgrounds[1].3 && !backgrounds[2].3);
        assert_eq!((backgrounds[0].0, backgrounds[0].1), (0, 2));
        assert_eq!(backgrounds[1].2, color(0x333333));
        assert_eq!(backgrounds[2].2, color(0x555555));
    }
}
