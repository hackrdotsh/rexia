//! One terminal's state on the client: the Ghostty terminal restored from the stream's
//! snapshot, Rex metadata applied in order, and immutable frames for the UI to draw.
//! Selection, search, scrollback and prompt navigation run here, where the terminal lives.
use super::data::{ApcParser, Part, SnapshotBuffer};
use anyhow::{Context, Result, ensure};
use ghostty_vt::{
    RenderState, Terminal,
    render::{CellIterator, CursorVisualStyle, RowIterator},
    screen::RowSemanticPrompt,
    search::Search,
    selection::{FormatOptions, SelectLineOptions, SelectWordOptions, Selection},
    style::{RgbColor, Underline},
    terminal::{Point, PointCoordinate, PointSpace, ScrollViewport},
};
use serde_json::Value;

pub mod flags {
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 1 << 1;
    pub const FAINT: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const DOUBLE_UNDERLINE: u16 = 1 << 4;
    pub const CURLY_UNDERLINE: u16 = 1 << 5;
    pub const STRIKE: u16 = 1 << 6;
    pub const OVERLINE: u16 = 1 << 7;
    pub const BLINK: u16 = 1 << 8;
    pub const SELECTED: u16 = 1 << 9;
    pub const MATCH: u16 = 1 << 10;
    pub const CURRENT_MATCH: u16 = 1 << 11;
    /// The first cell of a double-width character.
    pub const WIDE: u16 = 1 << 12;
    /// The cell a double-width character extends over; draw nothing in it.
    pub const SPACER: u16 = 1 << 13;
    pub const LINK: u16 = 1 << 14;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub fg: u32,
    /// None when the cell uses the terminal's default background.
    pub bg: Option<u32>,
    pub flags: u16,
}

impl Cell {
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    HollowBlock,
    Bar,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
    pub blinking: bool,
    pub color: Option<u32>,
    pub wide: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scrollbar {
    pub total: u64,
    pub offset: u64,
    pub visible: u64,
}

impl Scrollbar {
    pub fn at_bottom(&self) -> bool {
        self.offset + self.visible >= self.total
    }
}

/// Everything the UI needs to draw one terminal at one moment.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<Vec<Cell>>,
    pub fg: u32,
    pub bg: u32,
    pub cursor: Option<Cursor>,
    pub mouse_tracking: bool,
    pub alternate_screen: bool,
    pub scrollbar: Scrollbar,
    pub has_selection: bool,
    /// Match count and the selected match's index, while a search is open.
    pub search: Option<(usize, Option<usize>)>,
    /// The 16 ANSI colors, for chrome that matches the terminal.
    pub ansi: [u32; 16],
}

impl Frame {
    /// The plain text of one viewport row, without padding of wide characters.
    pub fn line_text(&self, row: usize) -> String {
        self.lines
            .get(row)
            .map(|cells| {
                cells
                    .iter()
                    .filter(|c| !c.has(flags::SPACER))
                    .map(|c| {
                        if c.text.is_empty() {
                            " "
                        } else {
                            c.text.as_str()
                        }
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .unwrap_or_default()
    }
}

/// Things programs did that the client may act on.
#[derive(Clone, Debug, PartialEq)]
pub enum TermEvent {
    Bell,
    Title(String),
    Pwd(String),
    Notification { title: String, body: String },
    Clipboard(String),
    ChildExited(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Cell,
    Word,
    Line,
    Rectangle,
}

pub fn rgb(color: RgbColor) -> u32 {
    (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

fn color(value: Option<&str>) -> Option<RgbColor> {
    let hex = value?.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(RgbColor {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    })
}

/// A terminal theme the client asks the server to apply (`set_theme`), or applies locally.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalTheme {
    pub foreground: u32,
    pub background: u32,
    pub cursor: Option<u32>,
    pub palette: Vec<(u8, u32)>,
}

pub struct TerminalState {
    // Declared before `terminal` so it's dropped first: it points into the terminal.
    search: Option<Search>,
    terminal: Option<Terminal>,
    snapshot: SnapshotBuffer,
    parser: ApcParser,
    render: RenderState,
    rows: RowIterator,
    cells: CellIterator,
    /// Bytes received since the stream started, for resuming at the same offset.
    pub received: u64,
    pub exit_code: Option<i64>,
    events: Vec<TermEvent>,
    /// The selection's fixed end, in screen coordinates (scrollback included).
    anchor: Option<(PointCoordinate, SelectMode)>,
    local_theme: Option<TerminalTheme>,
}

impl TerminalState {
    pub fn new(nonce: u64) -> Result<Self> {
        Ok(Self {
            search: None,
            terminal: None,
            snapshot: SnapshotBuffer::default(),
            parser: ApcParser::new(nonce),
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            received: 0,
            exit_code: None,
            events: Vec::new(),
            anchor: None,
            local_theme: None,
        })
    }

    pub fn ready(&self) -> bool {
        self.terminal.is_some()
    }

    pub fn terminal(&self) -> Option<&Terminal> {
        self.terminal.as_ref()
    }

    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// Feed bytes from the data channel. The first bytes complete the snapshot.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        self.received += bytes.len() as u64;
        let tail;
        let bytes = if self.terminal.is_none() {
            let Some((snapshot, rest)) = self.snapshot.push(bytes)? else {
                return Ok(());
            };
            let mut decoder = ghostty_vt::snapshot::Decoder::from_slice(&snapshot);
            decoder.ready()?;
            while decoder.next_page()?.is_some() {}
            ensure!(
                decoder.source_offset()? == Some(snapshot.len()),
                "terminal snapshot boundary mismatch"
            );
            let terminal = decoder
                .into_terminal()
                .context("terminal snapshot has no terminal")?;
            self.terminal = Some(terminal);
            if let Some(theme) = self.local_theme.clone() {
                self.apply_theme(&theme)?;
            }
            tail = rest;
            &tail[..]
        } else {
            bytes
        };
        for part in self.parser.feed(bytes)? {
            match part {
                Part::Output(output) => {
                    let terminal = self.terminal.as_mut().expect("snapshot restored");
                    terminal.vt_write(&output);
                }
                Part::Metadata(value) => self.metadata(&value)?,
            }
        }
        self.collect_events();
        if let (Some(search), Some(terminal)) = (self.search.as_mut(), self.terminal.as_ref()) {
            search.feed(terminal)?;
            search.run(terminal)?;
        }
        Ok(())
    }

    fn metadata(&mut self, value: &Value) -> Result<()> {
        let terminal = self.terminal.as_mut().expect("snapshot restored");
        match value["type"].as_str() {
            Some("resize") => {
                let dimension = |key: &str| {
                    value[key]
                        .as_u64()
                        .filter(|v| (1..=65535).contains(v))
                        .map(|v| v as u16)
                        .with_context(|| format!("invalid terminal resize {key}"))
                };
                terminal.resize(
                    dimension("cols")?,
                    dimension("rows")?,
                    value["cell_width_px"].as_u64().unwrap_or(0) as u32,
                    value["cell_height_px"].as_u64().unwrap_or(0) as u32,
                )?;
            }
            Some("theme") => {
                // A local theme wins over the server's: this client chose its colors.
                if self.local_theme.is_none() {
                    terminal.set_default_fg_color(color(value["foreground"].as_str()))?;
                    terminal.set_default_bg_color(color(value["background"].as_str()))?;
                    terminal.set_default_cursor_color(color(value["cursor"].as_str()))?;
                    terminal.set_default_color_palette(None)?;
                    if let Some(entries) = value["palette"].as_array() {
                        let mut palette = terminal.color_palette()?;
                        for entry in entries.iter().filter_map(Value::as_str) {
                            if let Some((index, hex)) = entry.split_once('=')
                                && let (Ok(index), Some(rgb)) =
                                    (index.parse::<u8>(), color(Some(hex)))
                            {
                                palette.0[index as usize] = rgb;
                            }
                        }
                        terminal.set_default_color_palette(Some(palette))?;
                    }
                }
            }
            Some("child_exited") => {
                self.exit_code = value["exit_code"].as_i64();
                if let Some(code) = self.exit_code {
                    self.events.push(TermEvent::ChildExited(code));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_events(&mut self) {
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        for event in terminal.events() {
            use ghostty_vt::terminal::Event;
            let event = match event {
                Event::Bell => TermEvent::Bell,
                Event::TitleChanged(title) => TermEvent::Title(title),
                Event::PwdChanged(pwd) => TermEvent::Pwd(pwd),
                Event::DesktopNotification { title, body } => {
                    TermEvent::Notification { title, body }
                }
                Event::ClipboardWrite(write) => {
                    let Some(text) = write
                        .contents
                        .iter()
                        .find(|c| c.mime.starts_with("text/plain"))
                    else {
                        continue;
                    };
                    TermEvent::Clipboard(String::from_utf8_lossy(&text.data).into_owned())
                }
                _ => continue,
            };
            if self.events.len() < 64 {
                self.events.push(event);
            }
        }
    }

    /// Use these colors instead of the server's (the user picked a local theme).
    pub fn set_local_theme(&mut self, theme: Option<TerminalTheme>) -> Result<()> {
        self.local_theme = theme.clone();
        if let Some(theme) = theme {
            self.apply_theme(&theme)?;
        }
        Ok(())
    }

    fn apply_theme(&mut self, theme: &TerminalTheme) -> Result<()> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        let to_rgb = |v: u32| RgbColor {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        };
        terminal.set_default_fg_color(Some(to_rgb(theme.foreground)))?;
        terminal.set_default_bg_color(Some(to_rgb(theme.background)))?;
        terminal.set_default_cursor_color(theme.cursor.map(to_rgb))?;
        terminal.set_default_color_palette(None)?;
        let mut palette = terminal.color_palette()?;
        for (index, value) in &theme.palette {
            palette.0[*index as usize] = to_rgb(*value);
        }
        terminal.set_default_color_palette(Some(palette))?;
        Ok(())
    }

    pub fn scroll(&mut self, scroll: ScrollViewport) {
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.scroll_viewport(scroll);
        }
    }

    /// Move the viewport to the previous (`-1`) or next (`1`) shell prompt (OSC 133).
    pub fn jump_to_prompt(&mut self, direction: i32) -> Result<bool> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(false);
        };
        let bar = terminal.scrollbar()?;
        let top = bar.offset as i64;
        let last = bar.total.saturating_sub(1) as i64;
        let mut y = top + direction as i64;
        // Bounded so a huge scrollback can't stall the UI.
        for _ in 0..50_000 {
            if y < 0 || y > last {
                return Ok(false);
            }
            let point = Point::Screen(PointCoordinate { x: 0, y: y as u32 });
            if let Ok(grid) = terminal.grid_ref(point)
                && grid.row()?.semantic_prompt()? == RowSemanticPrompt::Prompt
            {
                terminal.scroll_viewport(ScrollViewport::Row(y as usize));
                return Ok(true);
            }
            y += direction as i64;
        }
        Ok(false)
    }

    fn screen_point(terminal: &Terminal, x: u16, y: u16) -> Result<PointCoordinate> {
        let offset = terminal.scrollbar()?.offset;
        Ok(PointCoordinate {
            x,
            y: (offset + y as u64) as u32,
        })
    }

    /// Start a selection at a viewport cell.
    pub fn select_start(&mut self, x: u16, y: u16, mode: SelectMode) -> Result<()> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(());
        };
        let point = Self::screen_point(terminal, x, y)?;
        self.anchor = Some((point, mode));
        let grid = terminal.grid_ref(Point::Screen(point))?;
        let selection = match mode {
            SelectMode::Word => terminal.select_word(SelectWordOptions::new(grid))?,
            SelectMode::Line => terminal.select_line(SelectLineOptions::new(grid))?,
            SelectMode::Cell | SelectMode::Rectangle => None,
        };
        terminal.set_selection(selection.as_ref())?;
        Ok(())
    }

    /// Extend the selection from its anchor to a viewport cell.
    pub fn select_to(&mut self, x: u16, y: u16) -> Result<()> {
        let (Some(terminal), Some((anchor, mode))) = (self.terminal.as_ref(), self.anchor) else {
            return Ok(());
        };
        let head = Self::screen_point(terminal, x, y)?;
        let start = terminal.grid_ref(Point::Screen(anchor))?;
        let end = terminal.grid_ref(Point::Screen(head))?;
        let selection = match mode {
            SelectMode::Cell => Selection::new(start, end, false),
            SelectMode::Rectangle => Selection::new(start, end, true),
            SelectMode::Word | SelectMode::Line => {
                let (first, second) = if (anchor.y, anchor.x) <= (head.y, head.x) {
                    (start, end)
                } else {
                    (end, start)
                };
                let unit = |grid| {
                    if mode == SelectMode::Word {
                        terminal.select_word(SelectWordOptions::new(grid))
                    } else {
                        terminal.select_line(SelectLineOptions::new(grid))
                    }
                };
                let (first_point, second_point) = if (anchor.y, anchor.x) <= (head.y, head.x) {
                    (anchor, head)
                } else {
                    (head, anchor)
                };
                match (unit(first)?, unit(second)?) {
                    (Some(a), Some(b)) => Selection::new(a.start(), b.end(), false),
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => Selection::new(
                        terminal.grid_ref(Point::Screen(first_point))?,
                        terminal.grid_ref(Point::Screen(second_point))?,
                        false,
                    ),
                }
            }
        };
        terminal.set_selection(Some(&selection))?;
        Ok(())
    }

    pub fn select_all(&mut self) -> Result<()> {
        if let Some(terminal) = self.terminal.as_ref() {
            let all = terminal.select_all()?;
            terminal.set_selection(all.as_ref())?;
            self.anchor = None;
        }
        Ok(())
    }

    pub fn clear_selection(&mut self) -> Result<()> {
        self.anchor = None;
        if let Some(terminal) = self.terminal.as_ref() {
            terminal.set_selection(None)?;
        }
        Ok(())
    }

    /// The selected text, unwrapped and with trailing spaces trimmed.
    pub fn selection_text(&self) -> Result<Option<String>> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(None);
        };
        let Some(selection) = terminal.selection()? else {
            return Ok(None);
        };
        let options = FormatOptions::new()
            .with_selection(&selection)
            .with_unwrap(true)
            .with_trim(true);
        Ok(terminal
            .format_selection_alloc(options)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// The screen and scrollback as plain text.
    pub fn text(&self) -> Result<String> {
        let terminal = self
            .terminal
            .as_ref()
            .context("terminal not restored yet")?;
        let mut formatter = ghostty_vt::fmt::Formatter::new(
            terminal,
            ghostty_vt::fmt::FormatterOptions::new()
                .with_unwrap(true)
                .with_trim(true),
        )?;
        Ok(String::from_utf8_lossy(&formatter.format_alloc()?).into_owned())
    }

    /// The OSC 8 link at a viewport cell.
    pub fn link_at(&self, x: u16, y: u16) -> Option<String> {
        let terminal = self.terminal.as_ref()?;
        let grid = terminal
            .grid_ref(Point::Viewport(PointCoordinate { x, y: y as u32 }))
            .ok()?;
        let mut buffer = vec![0; 4096];
        let len = grid.hyperlink_uri(&mut buffer).ok()?;
        (len > 0).then(|| String::from_utf8_lossy(&buffer[..len.min(buffer.len())]).into_owned())
    }

    /// Search the screen and scrollback; `None` closes the search.
    pub fn search(&mut self, needle: Option<&str>) -> Result<()> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(());
        };
        match needle.filter(|n| !n.is_empty()) {
            None => self.search = None,
            Some(needle) => {
                if self.search.is_none() {
                    self.search = Some(Search::new(terminal)?);
                }
                let search = self.search.as_mut().expect("search");
                search.set_needle(terminal, Some(needle))?;
                search.feed(terminal)?;
                search.run(terminal)?;
                search.select_prev(terminal)?;
            }
        }
        Ok(())
    }

    /// Select the next older (`true`) or newer match, scrolling to it.
    pub fn search_step(&mut self, older: bool) -> Result<()> {
        if let (Some(search), Some(terminal)) = (self.search.as_mut(), self.terminal.as_ref()) {
            if older {
                search.select_next(terminal)?;
            } else {
                search.select_prev(terminal)?;
            }
        }
        Ok(())
    }

    pub fn frame(&mut self) -> Result<Option<Frame>> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(None);
        };
        let mut matches: Vec<(u32, u16, u32, u16, bool)> = Vec::new();
        let mut search_state = None;
        if let Some(search) = self.search.as_ref() {
            let current = search.selected_match(terminal)?;
            let mut push = |selection: &Selection<'_>, current: bool| -> Result<()> {
                let start =
                    terminal.point_from_grid_ref(&selection.start(), PointSpace::Viewport)?;
                let end = terminal.point_from_grid_ref(&selection.end(), PointSpace::Viewport)?;
                if let (Some(a), Some(b)) = (start, end) {
                    matches.push((a.y, a.x, b.y, b.x, current));
                }
                Ok(())
            };
            for selection in search.viewport_matches(terminal)? {
                push(&selection, false)?;
            }
            if let Some(selection) = current {
                push(&selection, true)?;
            }
            search_state = Some((search.total_matches()?, search.selected_index()?));
        }

        let snapshot = self.render.update(terminal)?;
        let colors = snapshot.colors()?;
        let fg = rgb(colors.foreground);
        let bg = rgb(colors.background);
        let mut ansi = [0; 16];
        for (index, slot) in ansi.iter_mut().enumerate() {
            *slot = rgb(colors.palette[index]);
        }
        let cursor = if snapshot.cursor_visible()? {
            snapshot.cursor_viewport()?.map(|at| Cursor {
                x: at.x,
                y: at.y,
                shape: match snapshot.cursor_visual_style() {
                    Ok(CursorVisualStyle::Bar) => CursorShape::Bar,
                    Ok(CursorVisualStyle::Underline) => CursorShape::Underline,
                    Ok(CursorVisualStyle::BlockHollow) => CursorShape::HollowBlock,
                    _ => CursorShape::Block,
                },
                blinking: snapshot.cursor_blinking().unwrap_or(false),
                color: snapshot.cursor_color().ok().flatten().map(rgb),
                wide: at.at_wide_tail,
            })
        } else {
            None
        };
        let bar = terminal.scrollbar()?;
        let mut frame = Frame {
            cols: snapshot.cols()?,
            rows: snapshot.rows()?,
            lines: Vec::new(),
            fg,
            bg,
            cursor,
            mouse_tracking: terminal.is_mouse_tracking()?,
            alternate_screen: terminal.active_screen()? == ghostty_vt::screen::Screen::Alternate,
            scrollbar: Scrollbar {
                total: bar.total,
                offset: bar.offset,
                visible: bar.len,
            },
            has_selection: terminal.selection()?.is_some(),
            search: search_state,
            ansi,
        };

        let mut rows = self.rows.update(&snapshot)?;
        let mut y: u32 = 0;
        while let Some(row) = rows.next() {
            let selected = row.selection()?;
            let mut cells = self.cells.update(row)?;
            let mut line = Vec::with_capacity(frame.cols as usize);
            let mut x: u16 = 0;
            while let Some(cell) = cells.next() {
                let mut text = String::new();
                cell.graphemes_utf8(&mut text)?;
                let style = cell.style()?;
                let raw = cell.raw_cell()?;
                let mut cell_fg = cell.fg_color()?.map(rgb).unwrap_or(fg);
                let mut cell_bg = cell.bg_color()?.map(rgb);
                if style.inverse {
                    let back = cell_bg.unwrap_or(bg);
                    cell_bg = Some(cell_fg);
                    cell_fg = back;
                }
                if style.invisible {
                    text.clear();
                }
                let mut bits = [
                    (style.bold, flags::BOLD),
                    (style.italic, flags::ITALIC),
                    (style.faint, flags::FAINT),
                    (style.strikethrough, flags::STRIKE),
                    (style.overline, flags::OVERLINE),
                    (style.blink, flags::BLINK),
                    (raw.has_hyperlink().unwrap_or(false), flags::LINK),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .fold(0, |bits, (_, flag)| bits | flag);
                match style.underline {
                    Underline::None => {}
                    Underline::Double => bits |= flags::DOUBLE_UNDERLINE,
                    Underline::Curly => bits |= flags::CURLY_UNDERLINE,
                    _ => bits |= flags::UNDERLINE,
                }
                match raw.wide()? {
                    ghostty_vt::screen::CellWide::Wide => bits |= flags::WIDE,
                    ghostty_vt::screen::CellWide::SpacerTail
                    | ghostty_vt::screen::CellWide::SpacerHead => bits |= flags::SPACER,
                    _ => {}
                }
                if let Some(range) = selected
                    && x >= range.start_x
                    && x <= range.end_x
                {
                    bits |= flags::SELECTED;
                }
                for &(sy, sx, ey, ex, current) in &matches {
                    let after_start = y > sy || (y == sy && x >= sx);
                    let before_end = y < ey || (y == ey && x <= ex);
                    if after_start && before_end {
                        bits |= if current {
                            flags::CURRENT_MATCH
                        } else {
                            flags::MATCH
                        };
                    }
                }
                line.push(Cell {
                    text,
                    fg: cell_fg,
                    bg: cell_bg,
                    flags: bits,
                });
                x += 1;
            }
            frame.lines.push(line);
            y += 1;
        }
        snapshot.clean()?;
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghostty_vt::TerminalOptions;

    fn stream_from(write: &[u8], cols: u16, rows: u16) -> TerminalState {
        let mut terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            ..Default::default()
        })
        .unwrap();
        terminal.vt_write(write);
        let snapshot = ghostty_vt::snapshot::encode(&terminal).unwrap();
        let mut state = TerminalState::new(42).unwrap();
        state.feed(&snapshot).unwrap();
        state
    }

    #[test]
    fn snapshot_then_output_then_metadata() {
        let mut state = stream_from(b"restored", 20, 4);
        state
            .feed(b"\r\n\x1b[1mlive\x1b[0m\x1b_rex1;42;{\"type\":\"resize\",\"cols\":30,\"rows\":5}\x1b\\")
            .unwrap();
        let frame = state.frame().unwrap().unwrap();
        assert_eq!((frame.cols, frame.rows), (30, 5));
        assert_eq!(frame.line_text(0), "restored");
        assert_eq!(frame.line_text(1), "live");
        assert!(frame.lines[1][0].has(flags::BOLD));
        assert!(!frame.lines[1][4].has(flags::BOLD));
    }

    #[test]
    fn wide_characters_and_colors() {
        let mut state = stream_from("界x\x1b[31mr\x1b[7mi".as_bytes(), 10, 2);
        let frame = state.frame().unwrap().unwrap();
        let line = &frame.lines[0];
        assert!(line[0].has(flags::WIDE));
        assert!(line[1].has(flags::SPACER));
        assert_eq!(line[2].text, "x");
        assert_ne!(line[3].fg, frame.fg, "red text");
        assert_eq!(
            line[4].bg,
            Some(line[3].fg),
            "inverse swaps red into the background"
        );
    }

    #[test]
    fn selection_copies_text_across_lines() {
        let mut state = stream_from(b"hello world\r\nsecond line", 20, 3);
        state.select_start(6, 0, SelectMode::Cell).unwrap();
        state.select_to(5, 1).unwrap();
        assert_eq!(state.selection_text().unwrap().unwrap(), "world\nsecond");
        let frame = state.frame().unwrap().unwrap();
        assert!(frame.has_selection);
        assert!(frame.lines[0][6].has(flags::SELECTED));
        assert!(!frame.lines[0][5].has(flags::SELECTED));
        state.select_start(1, 1, SelectMode::Word).unwrap();
        assert_eq!(state.selection_text().unwrap().unwrap(), "second");
        state.clear_selection().unwrap();
        assert!(state.selection_text().unwrap().is_none());
    }

    #[test]
    fn search_highlights_and_steps() {
        let mut state = stream_from(b"apple banana apple\r\ncherry apple", 30, 3);
        state.search(Some("apple")).unwrap();
        let frame = state.frame().unwrap().unwrap();
        assert_eq!(frame.search.unwrap().0, 3);
        let highlighted = frame
            .lines
            .iter()
            .flatten()
            .filter(|c| c.has(flags::MATCH) || c.has(flags::CURRENT_MATCH))
            .count();
        assert_eq!(highlighted, 15);
        let first = frame.search.unwrap().1;
        state.search_step(true).unwrap();
        assert_ne!(state.frame().unwrap().unwrap().search.unwrap().1, first);
        state.search(None).unwrap();
        assert!(state.frame().unwrap().unwrap().search.is_none());
    }

    #[test]
    fn scrollback_and_prompt_jumps() {
        let mut lines = Vec::new();
        for i in 0..40 {
            if i % 10 == 0 {
                lines.extend_from_slice(b"\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n");
            } else {
                lines.extend_from_slice(format!("output {i}\r\n").as_bytes());
            }
        }
        let mut state = stream_from(&lines, 20, 5);
        let bottom = state.frame().unwrap().unwrap().scrollbar;
        assert!(bottom.at_bottom());
        state.scroll(ScrollViewport::Delta(-10));
        assert!(!state.frame().unwrap().unwrap().scrollbar.at_bottom());
        state.scroll(ScrollViewport::Bottom);
        assert!(state.jump_to_prompt(-1).unwrap());
        let frame = state.frame().unwrap().unwrap();
        assert!(
            frame.line_text(0).starts_with("$ cmd"),
            "{:?}",
            frame.line_text(0)
        );
    }

    #[test]
    fn program_events_are_collected() {
        let mut state = stream_from(b"", 10, 2);
        state
            .feed(b"\x07\x1b]0;my title\x07\x1b]9;done building\x07\x1b_rex1;42;{\"type\":\"child_exited\",\"exit_code\":3}\x1b\\")
            .unwrap();
        let events = state.take_events();
        assert!(events.contains(&TermEvent::Bell));
        assert!(events.contains(&TermEvent::Title("my title".into())));
        assert!(
            events.iter().any(
                |e| matches!(e, TermEvent::Notification { body, .. } if body == "done building")
            )
        );
        assert!(events.contains(&TermEvent::ChildExited(3)));
        assert_eq!(state.exit_code, Some(3));
    }

    #[test]
    fn local_theme_overrides_server_theme() {
        let mut state = stream_from(b"x", 10, 2);
        state
            .set_local_theme(Some(TerminalTheme {
                foreground: 0xe2e2e2,
                background: 0x000000,
                cursor: None,
                palette: vec![(1, 0xffb4ab)],
            }))
            .unwrap();
        state
            .feed(b"\x1b_rex1;42;{\"type\":\"theme\",\"foreground\":\"#111111\",\"background\":\"#ffffff\"}\x1b\\\x1b[31mr")
            .unwrap();
        let frame = state.frame().unwrap().unwrap();
        assert_eq!((frame.fg, frame.bg), (0xe2e2e2, 0x000000));
        assert_eq!(frame.lines[0][1].fg, 0xffb4ab);
    }
}
