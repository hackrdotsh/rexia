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

gpui::actions!(
    terminal,
    [
        /// A key went to the terminal: handling this hides the pointer until the mouse
        /// moves, as GPUI does for keys an action takes. GPUI hides it by itself only for
        /// keys that type a character, not Return, Backspace, arrows or Ctrl combinations.
        HidePointer
    ]
);

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
    OpenPath(String),
    QuickLook,
    /// A paste that could run a command before it's read, waiting for a yes.
    ConfirmPaste(String),
    /// Files for a terminal on another machine, to upload there.
    Upload(Vec<crate::rex::upload::Source>),
    /// Right-click: the menu for the cell at `cell`, shown at `position` in the window.
    ContextMenu {
        position: gpui::Point<Pixels>,
        cell: (u16, u16),
    },
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
/// extend` does. Primary-screen edges must be uniform to avoid stretching colorful
/// shell prompts. Automatic also requires the alternate screen and uniform edges.
fn padding_fills(
    frame: &Frame,
    mode: PaddingFill,
    bounds: Bounds<Pixels>,
    origin: Point<Pixels>,
    cell: CellSize,
) -> Vec<(Bounds<Pixels>, u32)> {
    let uniform_edges = match mode {
        PaddingFill::Off => return Vec::new(),
        PaddingFill::Automatic if !frame.alternate_screen => return Vec::new(),
        PaddingFill::Automatic => true,
        PaddingFill::Always => !frame.alternate_screen,
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
    let edge = |colors: Vec<u32>| -> Option<Vec<u32>> {
        (!uniform_edges || uniform(colors.iter().copied()).is_some()).then_some(colors)
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

/// Cut a rectangle out without overlapping the remaining pieces.
fn outside(area: Bounds<Pixels>, cut: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    let cut = area.intersect(&cut);
    if cut.size.width <= px(0.) || cut.size.height <= px(0.) {
        return vec![area];
    }
    let right = area.origin.x + area.size.width;
    let bottom = area.origin.y + area.size.height;
    let x = cut.origin.x + cut.size.width;
    let y = cut.origin.y + cut.size.height;
    [
        Bounds::new(
            area.origin,
            size(area.size.width, cut.origin.y - area.origin.y),
        ),
        Bounds::new(point(area.origin.x, y), size(area.size.width, bottom - y)),
        Bounds::new(
            point(area.origin.x, cut.origin.y),
            size(cut.origin.x - area.origin.x, cut.size.height),
        ),
        Bounds::new(point(x, cut.origin.y), size(right - x, cut.size.height)),
    ]
    .into_iter()
    .filter(|b| b.size.width > px(0.) && b.size.height > px(0.))
    .collect()
}

/// A disjoint background plane. Explicit cells follow below-background images;
/// defaults and padding precede them, matching the terminal graphics protocol.
fn background_fills(
    frame: &Frame,
    mode: PaddingFill,
    bounds: Bounds<Pixels>,
    origin: Point<Pixels>,
    cell: CellSize,
) -> Vec<(Bounds<Pixels>, u32, bool)> {
    let rows = frame.lines.len().min(frame.rows as usize);
    let grid = Bounds::new(
        origin,
        size(
            cell.width * f32::from(frame.cols),
            cell.height * rows as f32,
        ),
    );
    let mut unfilled = outside(bounds, grid);
    let mut fills = Vec::new();
    for (area, rgb) in padding_fills(frame, mode, bounds, origin, cell) {
        let area = bounds.intersect(&area);
        if area.size.width <= px(0.) || area.size.height <= px(0.) {
            continue;
        }
        unfilled = unfilled
            .into_iter()
            .flat_map(|b| outside(b, area))
            .collect();
        fills.push((area, rgb, false));
    }
    fills.extend(unfilled.into_iter().map(|area| (area, frame.bg, false)));
    for (row, cells) in frame.lines.iter().take(rows).enumerate() {
        let mut start = 0;
        while start < frame.cols as usize {
            let bg = cells.get(start).and_then(|c| c.bg);
            let mut end = start + 1;
            while end < frame.cols as usize && cells.get(end).and_then(|c| c.bg) == bg {
                end += 1;
            }
            let area = Bounds::new(
                origin + point(cell.width * start as f32, cell.height * row as f32),
                size(cell.width * (end - start) as f32, cell.height),
            );
            fills.push((area, bg.unwrap_or(frame.bg), bg.is_some()));
            start = end;
        }
    }
    fills
}

const INACTIVE_DIM: f32 = 0.4;

/// Program colors stay opaque; only the terminal's default background is transparent.
pub fn background_opacity(rgb: u32, default: u32, opacity: f32) -> f32 {
    if rgb == default { opacity } else { 1. }
}

/// Dim RGB rather than alpha, so the desktop never leaks through glyphs or sprites.
fn dim_color(value: Hsla, background: Hsla, dimmed: bool) -> Hsla {
    if !dimmed {
        return value;
    }
    let mut value = gpui::Rgba::from(value);
    let background = gpui::Rgba::from(background);
    let amount = INACTIVE_DIM;
    value.r += (background.r - value.r) * amount;
    value.g += (background.g - value.g) * amount;
    value.b += (background.b - value.b) * amount;
    value.into()
}

/// One image's part, as GPUI draws it: the source rectangle cut out, in BGRA.
fn texture(placement: &Placement, dim: Option<u32>) -> Option<Arc<RenderImage>> {
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
            let channel = |v: u8, shift: u32| {
                dim.map_or(v, |rgb| {
                    let background = ((rgb >> shift) & 255) as f32;
                    (f32::from(v) + (background - f32::from(v)) * INACTIVE_DIM).round() as u8
                })
            };
            bgra.extend_from_slice(&[channel(p[2], 0), channel(p[1], 8), channel(p[0], 16), p[3]]);
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

/// How far the text moves down so it sits at the pane's bottom: the space left under
/// `rows` whole lines, with Bottom anchoring.
fn anchor_shift(
    anchor: crate::settings::TerminalAnchor,
    bounds: Bounds<Pixels>,
    padding: Point<Pixels>,
    cell: CellSize,
    rows: Option<u16>,
) -> Pixels {
    let (crate::settings::TerminalAnchor::Bottom, Some(rows)) = (anchor, rows) else {
        return px(0.);
    };
    let inner = bounds.size.height - padding.y * 2.;
    (inner - cell.height * f32::from(rows)).max(px(0.))
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
    texture_dim: Option<u32>,
    background_opacity: f32,
    dimmed: bool,
    host: HostHandle,
    pub session_id: String,
    pub block_id: String,
    focus: FocusHandle,
    font: TerminalFont,
    pub padding: Point<Pixels>,
    cell: Option<CellSize>,
    /// The display scale the cell was measured for: cells are whole device pixels.
    cell_scale: f32,
    /// The grid's top-left corner in window coordinates, from the last paint.
    origin: Point<Pixels>,
    /// The size last requested from the server.
    requested: Option<(u16, u16)>,
    /// The mouse button a program is receiving reports for.
    reporting: Option<&'static str>,
    selecting: bool,
    /// The cell a Ctrl-click went down on, to open its link when released there.
    link_press: Option<(u16, u16)>,
    pub copy_on_select: bool,
    /// Corrections to program colors.
    pub colors: TextColors,
    /// Whether a program's colors fill the padding.
    pub padding_fill: PaddingFill,
    /// How selections look.
    pub selection: crate::settings::SelectionLook,
    /// When the copy shine started, and how many copies have shone.
    shine: Option<std::time::Instant>,
    /// The cell the pointer was last over.
    pointer: Option<(u16, u16)>,
    /// Files from another app being dragged over the pane.
    dropping: Option<Vec<std::path::PathBuf>>,
    /// Where the text sits when the pane is taller than its whole lines.
    pub anchor: crate::settings::TerminalAnchor,
    /// Reflow text locally as the pane changes size, and the size last reflowed to.
    pub smooth_resizing: bool,
    reflowed: Option<(u16, u16)>,
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
    /// Keys sent to the terminal that asked GPUI to hide the pointer.
    pointer_hidden: usize,
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
    pub fn set_appearance(&mut self, opacity: f32, dimmed: bool, cx: &mut Context<Self>) {
        if self.background_opacity != opacity || self.dimmed != dimmed {
            self.background_opacity = opacity;
            self.dimmed = dimmed;
            cx.notify();
        }
    }

    pub fn new(
        host: HostHandle,
        session_id: String,
        block_id: String,
        font: TerminalFont,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            background_opacity: 1.,
            dimmed: false,
            texture_dim: None,
            host,
            session_id,
            block_id,
            focus: cx.focus_handle(),
            font,
            padding: point(px(8.), px(6.)),
            cell: None,
            cell_scale: 0.,
            origin: Point::default(),
            requested: None,
            reporting: None,
            selecting: false,
            link_press: None,
            copy_on_select: false,
            colors: TextColors::default(),
            padding_fill: PaddingFill::Off,
            selection: Default::default(),
            shine: None,
            size_hold: None,
            smooth_scrolling: true,
            smooth_resizing: true,
            anchor: Default::default(),
            pointer: None,
            dropping: None,
            reflowed: None,
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
            pointer_hidden: 0,
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

    /// The terminal's own background color now, as the program or palette sets it.
    pub fn background(&self) -> Option<u32> {
        Some(self.frame()?.bg)
    }

    pub fn default_colors(&self) -> Option<(u32, u32)> {
        let frame = self.frame()?;
        Some((frame.fg, frame.bg))
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
        let dim = frame.filter(|_| self.dimmed).map(|f| f.bg);
        if self.texture_dim != dim {
            for (_, stale) in self.textures.drain() {
                let _ = window.drop_image(stale);
            }
            self.texture_dim = dim;
        }
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
                None => match texture(placement, dim) {
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

    /// Whether the keyboard is on this terminal, for tests.
    pub fn has_focus(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    /// The cell the pointer was last over, for Quick Look.
    pub fn pointer_cell(&self) -> Option<(u16, u16)> {
        self.pointer
    }

    /// The pane's stream, for asking it about its text.
    pub fn handle(&self) -> Option<crate::rex::host::PaneHandle> {
        self.pane()
    }

    /// Where the grid's top-left corner is drawn, for tests.
    pub fn grid_origin(&self) -> Point<Pixels> {
        self.origin
    }

    /// The size last asked of the server, for tests.
    pub fn requested_size(&self) -> Option<(u16, u16)> {
        self.requested
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
            dimmed: self.dimmed,
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
        font.weight = FontWeight(self.font.weight);
        if !self.font.ligatures {
            font.features = gpui::FontFeatures::disable_ligatures();
        }
        if !self.font.fallbacks.is_empty() {
            font.fallbacks = Some(gpui::FontFallbacks::from_fonts(self.font.fallbacks.clone()));
        }
        font
    }

    fn measure(&mut self, window: &mut Window) -> CellSize {
        let scale = window.scale_factor();
        if let Some(cell) = self.cell
            && self.cell_scale == scale
        {
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
        let cell = cell_in_device_pixels(
            width + px(self.font.spacing),
            natural * self.font.line_height,
            scale,
        );
        self.cell = Some(cell);
        self.cell_scale = scale;
        cell
    }

    /// Keep the terminal's size until `until`, while panes move; it fits afterwards.
    pub fn hold_size_until(&mut self, until: std::time::Instant) {
        self.size_hold = Some(until);
    }

    /// Ask the server for the grid that fits `bounds`.
    fn fit(&mut self, bounds: Bounds<Pixels>, cell: CellSize) {
        let inner_w = bounds.size.width - self.padding.x * 2.;
        let inner_h = bounds.size.height - self.padding.y * 2.;
        let cols = (inner_w / cell.width).floor().max(2.) as u16;
        let rows = (inner_h / cell.height).floor().max(1.) as u16;
        let (cell_width, cell_height) = (
            f32::from(cell.width).round() as u16,
            f32::from(cell.height).round() as u16,
        );
        // Smooth resizing: the text reflows here on this frame, without waiting for the
        // server, even while panes move.
        if self.smooth_resizing
            && self.reflowed != Some((cols, rows))
            && let Some(pane) = self.pane()
        {
            self.reflowed = Some((cols, rows));
            pane.send(PaneCommand::Reflow {
                cols,
                rows,
                cell_width,
                cell_height,
            });
        }
        if self
            .size_hold
            .is_some_and(|t| std::time::Instant::now() < t)
            && self.requested.is_some()
        {
            return;
        }
        self.size_hold = None;
        if self.requested == Some((cols, rows)) {
            return;
        }
        self.requested = Some((cols, rows));
        self.host.send(HostCommand::Resize {
            session_id: self.session_id.clone(),
            block_id: self.block_id.clone(),
            cols,
            rows,
            cell_width,
            cell_height,
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

    /// How many keys sent to the terminal have hidden the pointer, for tests: the test
    /// platform doesn't track the pointer itself.
    pub fn pointer_hides(&self) -> usize {
        self.pointer_hidden
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
        if let Some(item) = cx.read_from_clipboard() {
            self.paste_item(item, cx);
        }
    }

    /// Paste what a clipboard holds: copied files as their paths, an image as a file, and
    /// text, after asking when it could run a command before it can be read.
    pub fn paste_item(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        use crate::rex::upload::{self, Source};
        for entry in item.entries() {
            if let gpui::ClipboardEntry::ExternalPaths(paths) = entry {
                let files = paths.paths().iter().cloned().map(Source::Path).collect();
                return self.paste_files(files, cx);
            }
        }
        if let Some(text) = item.text() {
            if let Some(paths) = upload::copied_files(&text) {
                return self.paste_files(paths.into_iter().map(Source::Path).collect(), cx);
            }
            let bracketed = self.frame().is_some_and(|f| f.bracketed_paste);
            if upload::unsafe_paste(&text, bracketed) {
                cx.emit(PaneEvent::ConfirmPaste(text));
            } else {
                self.paste_text(text);
            }
            return;
        }
        for entry in item.entries() {
            if let gpui::ClipboardEntry::Image(image) = entry {
                let name = format!("Pasted image.{}", image_extension(image.format));
                let bytes = std::sync::Arc::new(image.bytes.clone());
                return self.paste_files(vec![Source::Bytes { name, bytes }], cx);
            }
        }
    }

    /// Files dropped or pasted on the terminal: their paths typed, for a terminal on this
    /// machine; uploaded first, for one on another.
    pub fn paste_files(&mut self, files: Vec<crate::rex::upload::Source>, cx: &mut Context<Self>) {
        use crate::rex::upload::{Source, shell_quote};
        if files.is_empty() {
            return;
        }
        if !self.host.endpoint.is_local() {
            cx.emit(PaneEvent::Upload(files));
            return;
        }
        let mut paths = Vec::new();
        for file in files {
            match file {
                Source::Path(path) => paths.push(path),
                Source::Bytes { name, bytes } => match keep_pasted(&name, &bytes) {
                    Ok(path) => paths.push(path),
                    Err(error) => eprintln!("rexia: couldn't keep the pasted {name}: {error}"),
                },
            }
        }
        let text = paths
            .iter()
            .map(|p| shell_quote(&p.display().to_string()))
            .collect::<Vec<_>>()
            .join(" ");
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
        self.pointer = Some(at);
        if event.button == MouseButton::Left && event.modifiers.control && event.modifiers.shift {
            cx.emit(PaneEvent::QuickLook);
            return;
        }
        // Ctrl-click opens a program's OSC 8 link, as in Ghostty, once released on the same
        // cell: Ctrl-drag picks the pane up instead.
        if event.button == MouseButton::Left && event.modifiers.control && !event.modifiers.shift {
            self.link_press = Some(at);
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
            // A program using the mouse gets right-clicks; Shift always opens the menu.
            MouseButton::Right => cx.emit(PaneEvent::ContextMenu {
                position: event.position,
                cell: at,
            }),
            _ => {}
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let at = self.cell_at(event.position);
        self.pointer = Some(at);
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

    fn on_mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(paths) = self.dropping.take()
            && event.button == MouseButton::Left
            && cx.has_active_drag()
        {
            cx.stop_active_drag(window);
            let files = paths
                .into_iter()
                .map(crate::rex::upload::Source::Path)
                .collect();
            self.paste_files(files, cx);
            return;
        }
        let at = self.cell_at(event.position);
        if let Some(pressed) = self.link_press.take()
            && pressed == at
            && event.button == MouseButton::Left
            && let Some(pane) = self.pane()
        {
            let (x, y) = at;
            let frame = self.frame();
            let line = frame
                .as_ref()
                .map(|f| f.line_text(usize::from(y)))
                .unwrap_or_default();
            let text_column = frame
                .as_ref()
                .and_then(|f| f.lines.get(usize::from(y)))
                .map(|cells| {
                    cells
                        .iter()
                        .take(usize::from(x))
                        .filter(|c| !c.has(flags::SPACER))
                        .map(|c| c.text.chars().count().max(1))
                        .sum()
                })
                .unwrap_or(usize::from(x));
            cx.spawn(async move |this, cx| {
                let link = pane
                    .ask(|reply| PaneCommand::LinkAt { x, y, reply })
                    .await
                    .flatten();
                if let Some(link) = link {
                    let _ = this.update(cx, |_, cx| cx.emit(PaneEvent::OpenLink(link)));
                } else if let Some(target) = crate::quick_look::target_at(&line, text_column) {
                    let event = match target {
                        crate::quick_look::Target::Web(link) => PaneEvent::OpenLink(link),
                        crate::quick_look::Target::Path(path) => PaneEvent::OpenPath(path),
                    };
                    let _ = this.update(cx, |_, cx| cx.emit(event));
                }
            })
            .detach();
            return;
        }
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
    /// Shape single-cell Unicode separately so a wide fallback glyph stays in its cell.
    fit_cell: bool,
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
    dimmed: bool,
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

/// A cell `width` × `height` rounded to whole device pixels at `scale`, as Ghostty sizes
/// cells (`font/Metrics.zig` rounds both): every glyph then starts on a pixel boundary,
/// which keeps pixel fonts such as DepartureMono crisp, and the cell is the size Ghostty's
/// is. Rounding up instead would turn float noise (28.000002) into a whole extra pixel,
/// spacing braille unevenly between rows.
pub fn cell_in_device_pixels(width: Pixels, height: Pixels, scale: f32) -> CellSize {
    let device = |v: Pixels| px((f32::from(v) * scale).round().max(1.) / scale);
    CellSize {
        width: device(width),
        height: device(height),
    }
}

/// A character rexia draws itself (see `sprites`): its column, the character and its color.
type Sprite = (u16, char, Hsla);

fn segments(
    cells: &[crate::rex::stream::Cell],
    look: &Look,
) -> (Vec<Segment>, Vec<Background>, Vec<Sprite>) {
    let palette = &look.palette;
    let mut sprites: Vec<Sprite> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut backgrounds: Vec<Background> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        let col = col as u16;
        let mut fg = color(look.text_color(cell.fg, cell.bg));
        let mut bg = None;
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
        fg = dim_color(fg, color(look.default_bg), look.dimmed);
        bg = bg.map(|bg| dim_color(bg, color(look.default_bg), look.dimmed));
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
        let mut chars = cell.text.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && !cell.has(flags::WIDE)
            && super::sprites::is_sprite(c)
        {
            sprites.push((col, c, fg));
            continue;
        }
        // Nerd Fonts and Powerline use private-use glyphs that can deliberately
        // extend into adjacent whitespace. Keep their original size and overhang.
        let private_use = cell.text.chars().next().is_some_and(|c| {
            matches!(c, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
        });
        let fit_cell = !cell.has(flags::WIDE) && !cell.text.is_ascii() && !private_use;
        let alone = cell.has(flags::WIDE) || cell.text.chars().count() > 1 || fit_cell;
        if alone {
            segments.push(Segment {
                col,
                text: cell.text.clone(),
                runs: vec![(cell.text.len(), style)],
                forced: None,
                fit_cell,
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
                fit_cell: false,
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
    (segments, backgrounds, sprites)
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
        let opacity = self.background_opacity.clamp(0., 1.);
        let dimmed = self.dimmed;
        let dim = move |value| dim_color(value, background, dimmed);
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
        let anchor = self.anchor;
        let cursor_color = dim(color(theme.terminal.cursor));
        let cursor_text = dim(color(theme.terminal.cursor_text));
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
        let accent = dim(theme.primary);
        let preedit = self.preedit.clone().filter(|p| !p.is_empty() && focused);
        let focus_handle = self.focus.clone();
        let images = self.image_textures(frame.as_deref(), window);

        let grid = canvas(
            {
                let entity = entity.clone();
                move |bounds, window, cx| {
                    let scale = window.scale_factor();
                    entity.update(cx, |pane, _| {
                        let rows = pane.frame().map(|f| f.rows);
                        let at = bounds.origin
                            + padding
                            + point(px(0.), anchor_shift(anchor, bounds, padding, cell, rows));
                        // Where paint puts the grid: on a device pixel.
                        let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
                        pane.origin = point(snap(at.x), snap(at.y));
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
                let Some(frame) = frame else {
                    window.paint_quad(fill(bounds, background.opacity(opacity)));
                    return;
                };
                let shift = anchor_shift(anchor, bounds, padding, cell, Some(frame.rows));
                // The grid starts on a device pixel, so its cells do too.
                let scale = window.scale_factor();
                let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
                let origin = bounds.origin + padding + point(px(0.), scroll_shift + shift);
                let origin = point(snap(origin.x), snap(origin.y));
                let backgrounds = background_fills(&frame, padding_fill, bounds, origin, cell);
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
                if let Some(cursor) = frame.cursor {
                    let bounds =
                        cell_bounds(cursor.x, cursor.y as usize, if cursor.wide { 2 } else { 1 });
                    let color = cursor
                        .color
                        .map(super::theme::color)
                        .unwrap_or(cursor_color);
                    super::workspace::Workspace::record_focus_caret(
                        window,
                        cx,
                        &focus_handle,
                        bounds,
                        color,
                    );
                }
                let look = Look {
                    palette,
                    bold,
                    italic,
                    colors,
                    default_fg: frame.fg,
                    default_bg: frame.bg,
                    ansi: frame.ansi,
                    dimmed,
                };
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for &(area, rgb, explicit) in &backgrounds {
                        if !explicit {
                            let alpha = background_opacity(rgb, frame.bg, opacity);
                            window.paint_quad(fill(area, dim(color(rgb)).opacity(alpha)));
                        }
                    }
                    paint_images(Layer::BelowBackground, window);
                    for &(area, rgb, explicit) in &backgrounds {
                        if explicit {
                            let alpha = background_opacity(rgb, frame.bg, opacity);
                            window.paint_quad(fill(area, dim(color(rgb)).opacity(alpha)));
                        }
                    }
                    let mut rows = Vec::with_capacity(frame.lines.len());
                    let mut drawn = Vec::new();
                    for (row, cells) in frame.lines.iter().enumerate() {
                        let (segments, backgrounds, sprites) = segments(cells, &look);
                        drawn.extend(sprites.into_iter().map(|(col, c, fg)| (row, col, c, fg)));
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
                                        // Bold is heavier than the chosen weight.
                                        weight: if style.bold {
                                            FontWeight((terminal_font.weight.0 + 300.).min(900.))
                                        } else {
                                            terminal_font.weight
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
                            let text: SharedString = segment.text.into();
                            let mut line = window.text_system().shape_line(
                                text.clone(),
                                font_size,
                                &runs,
                                force,
                            );
                            let bounds = cell_bounds(segment.col, row, 1);
                            if segment.fit_cell {
                                if line.width > cell.width {
                                    let fitted_size = font_size * (cell.width / line.width);
                                    line = window.text_system().shape_line(
                                        text,
                                        fitted_size,
                                        &runs,
                                        None,
                                    );
                                }
                                window.with_content_mask(
                                    Some(gpui::ContentMask { bounds }),
                                    |window| {
                                        let _ = line.paint(
                                            bounds.origin,
                                            cell.height,
                                            TextAlign::Left,
                                            None,
                                            window,
                                            cx,
                                        );
                                    },
                                );
                            } else {
                                let _ = line.paint(
                                    bounds.origin,
                                    cell.height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                    // Braille and block elements, drawn to fit the cell exactly.
                    let scale = window.scale_factor();
                    let (cw, ch) = (
                        f32::from(cell.width) * scale,
                        f32::from(cell.height) * scale,
                    );
                    for (row, col, c, fg) in drawn {
                        let at = cell_bounds(col, row, 1).origin;
                        let (x0, y0) = (
                            (f32::from(at.x) * scale).round(),
                            (f32::from(at.y) * scale).round(),
                        );
                        for p in super::sprites::pieces(c, cw, ch) {
                            let area = Bounds::new(
                                point(px((x0 + p.x) / scale), px((y0 + p.y) / scale)),
                                size(px(p.w / scale), px(p.h / scale)),
                            );
                            window.paint_quad(fill(area, fg.opacity(fg.a * p.alpha)));
                        }
                    }
                    paint_images(Layer::AboveText, window);
                    // A cursor shader's trail, under the cursor it leads to.
                    if let Some((trail, t)) = &trail {
                        let color = frame
                            .cursor
                            .and_then(|c| c.color)
                            .map(super::theme::color)
                            .map(dim)
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
                            .map(dim)
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
                        let fg = dim(super::theme::color(frame.fg));
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
            .cursor(gpui::CursorStyle::IBeam)
            .on_key_down(cx.listener(|pane, event: &KeyDownEvent, window, cx| {
                if pane.send_key(event, cx) {
                    cx.stop_propagation();
                    window.dispatch_action(Box::new(HidePointer), cx);
                }
            }))
            .on_action(cx.listener(|pane, _: &HidePointer, _, _| pane.pointer_hidden += 1))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            // Files dragged in from other apps. GPUI's on_drop misses a drop that follows
            // typing (the pane counts as not hovered until the mouse moves), so the pane notes
            // the files while they're over it and takes them when the button comes up.
            .on_drag_move(cx.listener(
                |this, event: &gpui::DragMoveEvent<gpui::ExternalPaths>, _, cx| {
                    this.dropping = event
                        .bounds
                        .contains(&event.event.position)
                        .then(|| event.drag(cx).paths().to_vec());
                },
            ))
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

fn image_extension(format: gpui::ImageFormat) -> &'static str {
    use gpui::ImageFormat::*;
    match format {
        Png => "png",
        Jpeg => "jpg",
        Webp => "webp",
        Gif => "gif",
        Svg => "svg",
        Bmp => "bmp",
        Tiff => "tiff",
        _ => "img",
    }
}

/// Keep pasted bytes, such as a screenshot, in a file only this user can read, for a
/// terminal on this machine to take: under rexia's runtime directory, in a new folder so
/// the name stays as pasted.
fn keep_pasted(name: &str, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::rex::askpass::runtime_dir()?
        .join("pastes")
        .join(crate::rex::upload::uuid());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))?;
    Ok(path)
}

#[cfg(test)]
mod tests {

    #[test]
    fn cells_round_to_device_pixels_as_ghostty_does() {
        let device = |cell: CellSize, scale: f32| {
            (
                f32::from(cell.width) * scale,
                f32::from(cell.height) * scale,
            )
        };
        // DepartureMono 11 pt: 9.33 × 18.67 px, which GPUI's metrics make 28.000002 at 1.5×.
        let (width, height) = (px(9.333_333), px(14.666_667 + 4.));
        assert_eq!(
            device(cell_in_device_pixels(width, height, 1.5), 1.5),
            (14., 28.)
        );
        assert_eq!(
            device(cell_in_device_pixels(width, height, 2.), 2.),
            (19., 37.)
        );
        assert_eq!(
            device(cell_in_device_pixels(width, height, 1.), 1.),
            (9., 19.)
        );
    }
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
            bracketed_paste: false,
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
    fn mixed_shell_prompt_colors_do_not_extend_into_padding() {
        let red = Some(0xc02020);
        let blue = Some(0x2040c0);
        let prompt = [None, red, blue, None];
        let frame = frame_with(&[&prompt, &[None; 4], &prompt], false);
        let fills = padding_fills(
            &frame,
            PaddingFill::Always,
            Bounds::new(point(px(0.), px(0.)), size(px(60.), px(80.))),
            point(px(10.), px(10.)),
            CellSize {
                width: px(10.),
                height: px(20.),
            },
        );
        assert!(!fills.is_empty());
        assert!(fills.iter().all(|(_, color)| *color == frame.bg));
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
        // A shell: Automatic leaves it alone; Always extends only uniform edges.
        let shell = frame_with(&[&[red, None], &[blue, None]], false);
        assert!(colors(&shell, PaddingFill::Automatic).is_empty());
        assert_eq!(colors(&shell, PaddingFill::Always), vec![0x000000]);
        let fullscreen = frame_with(&[&[red, None], &[blue, None]], true);
        assert_eq!(
            colors(&fullscreen, PaddingFill::Always),
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
            dimmed: false,
        }
    }

    #[test]
    fn single_cell_unicode_is_fitted_without_resizing_wide_cells() {
        let row = [
            cell("a", 0),
            cell("⌘", 0),
            cell("界", flags::WIDE),
            cell("", flags::SPACER),
            cell("b", 0),
        ];
        let (segments, _, _) = segments(&row, &look());
        let cells: Vec<_> = segments.iter().map(|s| (s.col, s.fit_cell)).collect();
        assert_eq!(cells, [(0, false), (1, true), (2, false), (4, false)]);
    }

    #[test]
    fn prompt_icons_keep_their_original_shaping() {
        let row = [
            cell("\u{f313}", 0),
            cell("\u{e0b0}", 0),
            cell("\u{f0001}", 0),
            cell("a", 0),
        ];
        let (segments, _, _) = segments(&row, &look());
        assert_eq!(
            segments.len(),
            1,
            "prompt icons remain in the normal text run"
        );
        assert!(!segments[0].fit_cell);
        assert_eq!(segments[0].forced, Some(4));
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
        let (segments, backgrounds, _) = segments(&row, &look());
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
        let (_, backgrounds, _) = segments(&row, &look());
        assert_eq!(backgrounds.len(), 2);
        assert!(backgrounds[0].3 && !backgrounds[1].3);
        assert_eq!((backgrounds[0].0, backgrounds[0].1), (2, 3));
        assert_eq!(backgrounds[0].2, color(0x333333));
        assert_eq!(backgrounds[1].2, color(0x555555));
    }

    #[test]
    fn background_plane_covers_each_pixel_once() {
        let mut frame = frame_with(&[&[None, None, None], &[None, None, None]], true);
        frame.lines[0][0].bg = Some(0xff0000);
        frame.lines[1][1].bg = Some(0x0000ff);
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(80.), px(65.)));
        let cell = CellSize {
            width: px(10.),
            height: px(12.),
        };
        for mode in [
            PaddingFill::Off,
            PaddingFill::Automatic,
            PaddingFill::Always,
        ] {
            for origin in [
                point(px(8.), px(6.)),
                point(px(8.), px(-3.)),
                point(px(8.), px(23.)),
            ] {
                let fills = background_fills(&frame, mode, bounds, origin, cell);
                for y in 0..65 {
                    for x in 0..80 {
                        let at = point(px(x as f32 + 0.5), px(y as f32 + 0.5));
                        let count = fills.iter().filter(|(b, _, _)| b.contains(&at)).count();
                        assert_eq!(count, 1, "{mode:?} at {origin:?}: pixel {x},{y}");
                    }
                }
                let at = origin + point(px(5.), px(5.));
                assert!(
                    fills
                        .iter()
                        .any(|(b, rgb, explicit)| b.contains(&at) && *rgb == 0xff0000 && *explicit)
                );
            }
        }
    }

    #[test]
    fn dimming_preserves_foreground_alpha() {
        let mut look = look();
        look.dimmed = true;
        let (_, _, sprites) = segments(&[cell("▀", 0)], &look);
        let fg = gpui::Rgba::from(sprites[0].2);
        assert_eq!(fg.a, 1.);
        assert!((fg.r - 0.6).abs() < 0.001);
    }

    #[test]
    fn opacity_only_applies_to_the_default_background_color() {
        for default in [0x000000, 0xffffff, 0x203040] {
            for opacity in [0., 0.25, 0.5, 1.] {
                assert_eq!(background_opacity(default, default, opacity), opacity);
                assert_eq!(background_opacity(0xe37a1c, default, opacity), 1.);
            }
        }
    }
}
