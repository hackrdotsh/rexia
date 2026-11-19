//! Keyboard selection operates on Ghostty's full screen, not on the rendered viewport.
use anyhow::Result;
use ghostty_vt::{
    Terminal,
    screen::{CellWide, Screen, TrackedGridRef},
    selection::Selection,
    terminal::{Point, PointCoordinate, PointSpace, ScrollViewport},
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    BigWordForward,
    BigWordBackward,
    BigWordEnd,
    LineStart,
    FirstNonBlank,
    LineEnd,
    ViewportTop,
    ViewportMiddle,
    ViewportBottom,
    HalfPageUp,
    HalfPageDown,
    PageUp,
    PageDown,
    HistoryTop,
    HistoryBottom,
    ParagraphUp,
    ParagraphDown,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionKind {
    Character,
    Line,
    #[serde(alias = "rectangle")]
    Block,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Enter,
    Exit,
    Move(Motion),
    Select(SelectionKind),
    SwapEnds,
    Count(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub column: u16,
    pub selection: Option<SelectionKind>,
    pub count: Option<u16>,
}

pub(super) struct CopyMode {
    screen: Screen,
    cursor: TrackedGridRef,
    origin: Option<TrackedGridRef>,
    anchor: Option<(TrackedGridRef, SelectionKind)>,
    column: Option<u16>,
    count: Option<u16>,
}

impl CopyMode {
    pub(super) fn enter(terminal: &mut Terminal) -> Result<Self> {
        let bar = terminal.scrollbar()?;
        let at = PointCoordinate {
            x: 0,
            y: bar.offset as u32,
        };
        let origin = if bar.offset + bar.len >= bar.total {
            None
        } else {
            Some(terminal.track_grid_ref(Point::Screen(at))?)
        };
        let active = Point::Active(PointCoordinate {
            x: terminal.cursor_x()?,
            y: u32::from(terminal.cursor_y()?),
        });
        let grid = terminal.grid_ref(active)?;
        let point = if terminal
            .point_from_grid_ref(&grid, PointSpace::Viewport)?
            .is_some()
        {
            terminal
                .point_from_grid_ref(&grid, PointSpace::Screen)?
                .unwrap_or(at)
        } else {
            let y = bar.offset + bar.len.saturating_sub(1);
            PointCoordinate { x: 0, y: y as u32 }
        };
        let point = snap(terminal, point, false)?;
        terminal.set_selection(None)?;
        let mut mode = Self {
            screen: terminal.active_screen()?,
            cursor: terminal.track_grid_ref(Point::Screen(point))?,
            origin,
            anchor: None,
            column: None,
            count: None,
        };
        mode.reveal(terminal)?;
        Ok(mode)
    }

    pub(super) fn exit(self, terminal: &mut Terminal) -> Result<()> {
        terminal.set_selection(None)?;
        if terminal.active_screen()? == self.screen {
            let scroll = match self.origin {
                Some(origin) => origin
                    .point(PointSpace::Screen)?
                    .map_or(ScrollViewport::Top, |at| ScrollViewport::Row(at.y as usize)),
                None => ScrollViewport::Bottom,
            };
            terminal.scroll_viewport(scroll);
        }
        Ok(())
    }

    pub(super) fn sync(&mut self, terminal: &mut Terminal) -> Result<()> {
        if terminal.active_screen()? != self.screen || !self.cursor.has_value() {
            *self = Self::enter(terminal)?;
        }
        self.reveal(terminal)
    }

    fn point(&self) -> Result<PointCoordinate> {
        Ok(self
            .cursor
            .point(PointSpace::Screen)?
            .unwrap_or(PointCoordinate { x: 0, y: 0 }))
    }

    pub(super) fn position(&self) -> Result<Position> {
        let at = self.point()?;
        Ok(Position {
            line: at.y + 1,
            column: at.x + 1,
            selection: self.anchor.as_ref().map(|(_, kind)| *kind),
            count: self.count,
        })
    }

    pub(super) fn viewport_cursor(&self, terminal: &Terminal) -> Result<Option<super::Cursor>> {
        let Some(at) = self.cursor.point(PointSpace::Viewport)? else {
            return Ok(None);
        };
        let cell = terminal.grid_ref(Point::Viewport(at))?.cell()?;
        Ok(Some(super::Cursor {
            x: at.x,
            y: at.y as u16,
            shape: super::CursorShape::Block,
            blinking: false,
            color: None,
            wide: cell.wide()? == CellWide::Wide,
        }))
    }

    fn reveal(&mut self, terminal: &mut Terminal) -> Result<()> {
        let y = u64::from(self.point()?.y);
        let bar = terminal.scrollbar()?;
        let top = if y < bar.offset {
            y
        } else if y >= bar.offset + bar.len {
            y + 1 - bar.len
        } else {
            return Ok(());
        };
        terminal.scroll_viewport(ScrollViewport::Row(top as usize));
        Ok(())
    }

    pub(super) fn apply(&mut self, terminal: &mut Terminal, command: Command) -> Result<()> {
        self.sync(terminal)?;
        match command {
            Command::Move(motion) => {
                let count = self.count.take().unwrap_or(1);
                self.move_cursor(terminal, motion, count)?;
            }
            Command::Count(0) if self.count.is_none() => {
                self.move_cursor(terminal, Motion::LineStart, 1)?;
            }
            Command::Count(digit @ 0..=9) => {
                let prefix = self.count.unwrap_or(0);
                self.count = Some((prefix * 10 + u16::from(digit)).min(999));
            }
            Command::Select(kind) => {
                self.count = None;
                self.anchor = match self.anchor.take() {
                    Some((_, old)) if old == kind => None,
                    Some((anchor, _)) => Some((anchor, kind)),
                    None => Some((terminal.track_grid_ref(Point::Screen(self.point()?))?, kind)),
                };
            }
            Command::SwapEnds => {
                self.count = None;
                self.column = None;
                if let Some((anchor, _)) = &mut self.anchor {
                    std::mem::swap(anchor, &mut self.cursor);
                }
            }
            _ => {}
        }
        self.reveal(terminal)?;
        self.update_selection(terminal)
    }

    fn move_cursor(&mut self, terminal: &mut Terminal, motion: Motion, count: u16) -> Result<()> {
        use Motion::*;
        let grid = Grid::new(terminal)?;
        let mut at = snap(terminal, self.point()?, false)?;
        let distance = i64::from(count);
        let vertical = match motion {
            Up => -distance,
            Down => distance,
            HalfPageUp => -distance * i64::from((grid.height / 2).max(1)),
            HalfPageDown => distance * i64::from((grid.height / 2).max(1)),
            PageUp => -distance * i64::from(grid.height),
            PageDown => distance * i64::from(grid.height),
            _ => 0,
        };
        if vertical != 0 {
            at.x = *self.column.get_or_insert(at.x);
            at.y = (i64::from(at.y) + vertical).clamp(0, i64::from(grid.last_row)) as u32;
        } else {
            self.column = None;
            match motion {
                Left | Right => {
                    for _ in 0..count {
                        let x = if matches!(motion, Left) {
                            at.x.saturating_sub(1)
                        } else {
                            at.x.saturating_add(1).min(grid.last_col)
                        };
                        at.x = x;
                        at = snap(terminal, at, matches!(motion, Right))?;
                    }
                }
                LineStart => at.x = 0,
                FirstNonBlank => at.x = grid.first_nonblank(at.y)?,
                LineEnd => at.x = grid.last_nonblank(at.y)?,
                ViewportTop => at.y = grid.top,
                ViewportMiddle => at.y = grid.top + grid.height / 2,
                ViewportBottom => at.y = grid.top + grid.height.saturating_sub(1),
                HistoryTop => at = PointCoordinate { x: 0, y: 0 },
                HistoryBottom => {
                    at = PointCoordinate {
                        x: 0,
                        y: grid.last_row,
                    }
                }
                WordForward | WordBackward | WordEnd | BigWordForward | BigWordBackward
                | BigWordEnd => at = grid.word(at, motion, count)?,
                ParagraphUp | ParagraphDown => {
                    at = grid.paragraph(at, matches!(motion, ParagraphDown), count)?;
                }
                _ => {}
            }
        }
        at.y = at.y.min(grid.last_row);
        self.cursor
            .set(terminal, Point::Screen(snap(terminal, at, false)?))?;
        Ok(())
    }

    fn update_selection(&self, terminal: &Terminal) -> Result<()> {
        let Some((anchor, kind)) = &self.anchor else {
            terminal.set_selection(None)?;
            return Ok(());
        };
        let mut head = self.point()?;
        let Some(mut anchor) = anchor.point(PointSpace::Screen)? else {
            terminal.set_selection(None)?;
            return Ok(());
        };
        if (anchor.y, anchor.x) > (head.y, head.x) {
            std::mem::swap(&mut anchor, &mut head);
        }
        if *kind == SelectionKind::Line {
            anchor.x = 0;
            head.x = terminal.cols()?.saturating_sub(1);
        }
        set_selection(terminal, anchor, head, *kind == SelectionKind::Block)
    }

    pub(super) fn select_to_line_end(&self, terminal: &Terminal) -> Result<()> {
        let from = self.point()?;
        let to = PointCoordinate {
            x: Grid::new(terminal)?.last_nonblank(from.y)?.max(from.x),
            ..from
        };
        set_selection(terminal, from, to, false)
    }
}

fn set_selection(
    terminal: &Terminal,
    first: PointCoordinate,
    last: PointCoordinate,
    block: bool,
) -> Result<()> {
    let first = terminal.grid_ref(Point::Screen(first))?;
    let last = terminal.grid_ref(Point::Screen(last))?;
    terminal.set_selection(Some(&Selection::new(first, last, block)))?;
    Ok(())
}

fn snap(terminal: &Terminal, mut at: PointCoordinate, right: bool) -> Result<PointCoordinate> {
    let last = terminal.cols()?.saturating_sub(1);
    at.x = at.x.min(last);
    match terminal.grid_ref(Point::Screen(at))?.cell()?.wide()? {
        CellWide::SpacerTail if right && at.x < last => at.x += 1,
        CellWide::SpacerTail | CellWide::SpacerHead => at.x = at.x.saturating_sub(1),
        _ => {}
    }
    Ok(at)
}

struct Grid<'a> {
    terminal: &'a Terminal,
    top: u32,
    height: u32,
    last_row: u32,
    last_col: u16,
}

impl<'a> Grid<'a> {
    fn new(terminal: &'a Terminal) -> Result<Self> {
        let bar = terminal.scrollbar()?;
        Ok(Self {
            terminal,
            top: bar.offset as u32,
            height: bar.len as u32,
            last_row: bar.total.saturating_sub(1) as u32,
            last_col: terminal.cols()?.saturating_sub(1),
        })
    }

    fn character(&self, at: PointCoordinate) -> Result<Option<char>> {
        let cell = self.terminal.grid_ref(Point::Screen(at))?.cell()?;
        match cell.wide()? {
            CellWide::SpacerHead | CellWide::SpacerTail => Ok(None),
            _ => Ok(Some(
                char::from_u32(cell.codepoint()?)
                    .filter(|&c| c != '\0')
                    .unwrap_or(' '),
            )),
        }
    }

    fn first_nonblank(&self, y: u32) -> Result<u16> {
        for x in 0..=self.last_col {
            if self
                .character(PointCoordinate { x, y })?
                .is_some_and(|c| !c.is_whitespace())
            {
                return Ok(x);
            }
        }
        Ok(0)
    }

    fn last_nonblank(&self, y: u32) -> Result<u16> {
        for x in (0..=self.last_col).rev() {
            if self
                .character(PointCoordinate { x, y })?
                .is_some_and(|c| !c.is_whitespace())
            {
                return Ok(x);
            }
        }
        Ok(0)
    }

    fn blank_row(&self, y: u32) -> Result<bool> {
        Ok(self.last_nonblank(y)? == 0
            && self
                .character(PointCoordinate { x: 0, y })?
                .is_none_or(char::is_whitespace))
    }

    fn next_cell(&self, at: PointCoordinate, forward: bool) -> Option<PointCoordinate> {
        let PointCoordinate { x, y } = at;
        if forward {
            if x < self.last_col {
                Some(PointCoordinate { x: x + 1, y })
            } else {
                (y < self.last_row).then(|| PointCoordinate { x: 0, y: y + 1 })
            }
        } else if x > 0 {
            Some(PointCoordinate { x: x - 1, y })
        } else {
            y.checked_sub(1).map(|y| PointCoordinate {
                x: self.last_col,
                y,
            })
        }
    }

    fn word(
        &self,
        mut at: PointCoordinate,
        motion: Motion,
        mut count: u16,
    ) -> Result<PointCoordinate> {
        use Motion::*;
        let forward = !matches!(motion, WordBackward | BigWordBackward);
        let next_start = matches!(motion, WordForward | BigWordForward);
        let big = matches!(motion, BigWordForward | BigWordBackward | BigWordEnd);
        let class = |c: char| {
            if c.is_whitespace() {
                0
            } else if big || c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        let mut previous = class(self.character(at)?.unwrap_or(' '));
        let mut target = at;
        // Bound scans through mostly blank history so neither holds the stream task indefinitely.
        for _ in 0..50_000 {
            let Some(next) = self.next_cell(at, forward) else {
                if !next_start && previous != 0 {
                    target = at;
                }
                break;
            };
            let Some(c) = self.character(next)? else {
                at = next;
                continue;
            };
            let current = class(c);
            let wrapped = if at.y != next.y {
                let next_line = at.y.max(next.y);
                self.terminal
                    .grid_ref(Point::Screen(PointCoordinate { x: 0, y: next_line }))?
                    .row()?
                    .is_wrap_continuation()?
            } else {
                true
            };
            if previous != current || !wrapped {
                if next_start && current != 0 {
                    target = next;
                    count -= 1;
                } else if !next_start && previous != 0 && at != target {
                    target = at;
                    count -= 1;
                }
                if count == 0 {
                    break;
                }
            }
            at = next;
            previous = current;
        }
        snap(self.terminal, target, false)
    }

    fn paragraph(
        &self,
        mut at: PointCoordinate,
        forward: bool,
        mut count: u16,
    ) -> Result<PointCoordinate> {
        let mut was_blank = self.blank_row(at.y)?;
        for _ in 0..2048 {
            let next = if forward {
                (at.y < self.last_row).then(|| at.y + 1)
            } else {
                at.y.checked_sub(1)
            };
            let Some(y) = next else { break };
            at = PointCoordinate { x: 0, y };
            let blank = self.blank_row(y)?;
            if blank && !was_blank {
                count -= 1;
                if count == 0 {
                    break;
                }
            }
            was_blank = blank;
        }
        Ok(at)
    }
}
