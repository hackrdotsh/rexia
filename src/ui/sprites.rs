//! Cell-sized glyphs drawn without the font, as in Ghostty. Box drawing and blocks
//! meet at cell edges even with extra line spacing, and braille draws only set dots.

mod box_drawing;

use box_drawing::BoxDrawing;
use gpui::{Bounds, Hsla, Pixels, Point, Window, fill, point, px, size};

pub(super) enum Sprite {
    Box(BoxDrawing),
    Braille(u8),
    Block(u32),
}

impl Sprite {
    pub(super) fn from_text(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let code = c as u32;
        match code {
            0x2800..=0x28FF => Some(Self::Braille((code - 0x2800) as u8)),
            0x2580..=0x259F => Some(Self::Block(code)),
            _ => BoxDrawing::from_char(c).map(Self::Box),
        }
    }

    pub(super) fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    pub(super) fn paint(self, area: Bounds<Pixels>, light: f32, fg: Hsla, window: &mut Window) {
        let cell = DeviceCell::new(area, window.scale_factor());
        let (w, h) = (cell.width, cell.height);
        let pieces = match self {
            Self::Box(drawing) => return drawing.paint(cell, light, fg, window),
            Self::Braille(bits) => braille(bits, w, h),
            Self::Block(code) => block(code, w, h),
        };
        for p in pieces {
            cell.paint(p, fg.opacity(fg.a), window);
        }
    }
}

struct DeviceCell {
    origin: Point<f32>,
    width: f32,
    height: f32,
    scale: f32,
}

impl DeviceCell {
    fn new(bounds: Bounds<Pixels>, scale: f32) -> Self {
        Self {
            origin: bounds.origin.map(|v| (f32::from(v) * scale).round()),
            width: (f32::from(bounds.size.width) * scale).round(),
            height: (f32::from(bounds.size.height) * scale).round(),
            scale,
        }
    }

    fn point(&self, x: f32, y: f32) -> Point<Pixels> {
        point(
            px((self.origin.x + x) / self.scale),
            px((self.origin.y + y) / self.scale),
        )
    }

    fn paint(&self, p: Piece, fg: Hsla, window: &mut Window) {
        if p.w > 0. && p.h > 0. {
            let area = Bounds::new(
                self.point(p.x, p.y),
                size(px(p.w / self.scale), px(p.h / self.scale)),
            );
            window.paint_quad(fill(area, fg.opacity(p.alpha)));
        }
    }
}

/// A filled rectangle within a cell, in pixels from the cell's top-left, with its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    alpha: f32,
}

fn piece(x: f32, y: f32, w: f32, h: f32) -> Piece {
    Piece {
        x,
        y,
        w,
        h,
        alpha: 1.,
    }
}

/// Square dots on a 2 × 4 grid, sized and spaced as Ghostty's sprite font does
/// (`font/sprite/draw/braille.zig`): dots a quarter of the width or an eighth of the
/// height, gaps the same, and leftover pixels go to margins and gaps before the dots, so the
/// dots keep clear space between them.
fn braille(bits: u8, width: f32, height: f32) -> Vec<Piece> {
    let w = width as i32;
    let h = height as i32;
    let mut dot = (w / 4).min(h / 8);
    let mut x_gap = w / 4;
    let mut y_gap = h / 8;
    let mut x_margin = x_gap / 2;
    let mut y_margin = y_gap / 2;
    let mut x_left = w - 2 * x_margin - x_gap - 2 * dot;
    let mut y_left = h - 2 * y_margin - 3 * y_gap - 4 * dot;
    // First, dots that show at all.
    if x_left >= 2 && y_left >= 4 && dot == 0 {
        dot += 1;
        x_left -= 2;
        y_left -= 4;
    }
    // Then a margin, rather than dots against the cell's edge.
    if x_left >= 2 && x_margin == 0 {
        x_margin = 1;
        x_left -= 2;
    }
    if y_left >= 2 && y_margin == 0 {
        y_margin = 1;
        y_left -= 2;
    }
    // Then wider gaps between the dots.
    if x_left >= 1 {
        x_gap += 1;
        x_left -= 1;
    }
    if y_left >= 3 {
        y_gap += 1;
        y_left -= 3;
    }
    // Then wider margins.
    if x_left >= 2 {
        x_margin += 1;
        x_left -= 2;
    }
    if y_left >= 2 {
        y_margin += 1;
        y_left -= 2;
    }
    // Last, bigger dots.
    if x_left >= 2 && y_left >= 4 {
        dot += 1;
    }
    let dot = dot.max(1);
    // Dots 1-3 and 7 run down the left, 4-6 and 8 down the right.
    const DOTS: [(i32, i32); 8] = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    DOTS.iter()
        .enumerate()
        .filter(|(bit, _)| bits & (1 << bit) != 0)
        .map(|(_, (col, row))| {
            piece(
                (x_margin + col * (dot + x_gap)) as f32,
                (y_margin + row * (dot + y_gap)) as f32,
                dot as f32,
                dot as f32,
            )
        })
        .collect()
}

fn block(code: u32, w: f32, h: f32) -> Vec<Piece> {
    // Fractions round to whole pixels, so neighbors meet without seams.
    let across = |n: f32| (w * n / 8.).round();
    let down = |n: f32| (h * n / 8.).round();
    let (hw, hh) = ((w / 2.).round(), (h / 2.).round());
    let quadrants = |ul: bool, ur: bool, ll: bool, lr: bool| {
        let mut out = Vec::new();
        if ul {
            out.push(piece(0., 0., hw, hh));
        }
        if ur {
            out.push(piece(hw, 0., w - hw, hh));
        }
        if ll {
            out.push(piece(0., hh, hw, h - hh));
        }
        if lr {
            out.push(piece(hw, hh, w - hw, h - hh));
        }
        out
    };
    let shade = |alpha: f32| {
        vec![Piece {
            alpha,
            ..piece(0., 0., w, h)
        }]
    };
    match code {
        0x2580 => vec![piece(0., 0., w, hh)],
        // Lower one eighth to seven eighths, then full.
        0x2581..=0x2588 => {
            let top = h - down((code - 0x2580) as f32);
            vec![piece(0., top, w, h - top)]
        }
        // Left seven eighths down to one eighth.
        0x2589..=0x258F => vec![piece(0., 0., across((0x2590 - code) as f32), h)],
        0x2590 => vec![piece(hw, 0., w - hw, h)],
        0x2591 => shade(0.25),
        0x2592 => shade(0.5),
        0x2593 => shade(0.75),
        0x2594 => vec![piece(0., 0., w, down(1.))],
        0x2595 => {
            let left = w - across(1.);
            vec![piece(left, 0., w - left, h)]
        }
        0x2596 => quadrants(false, false, true, false),
        0x2597 => quadrants(false, false, false, true),
        0x2598 => quadrants(true, false, false, false),
        0x2599 => quadrants(true, false, true, true),
        0x259A => quadrants(true, false, false, true),
        0x259B => quadrants(true, true, true, false),
        0x259C => quadrants(true, true, false, true),
        0x259D => quadrants(false, true, false, false),
        0x259E => quadrants(false, true, true, false),
        0x259F => quadrants(false, true, true, true),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(c: char, width: f32, height: f32) -> Vec<Piece> {
        match Sprite::from_text(c.encode_utf8(&mut [0; 4])) {
            Some(Sprite::Braille(bits)) => braille(bits, width, height),
            Some(Sprite::Block(code)) => block(code, width, height),
            _ => Vec::new(),
        }
    }

    #[test]
    fn braille_draws_only_the_dots_that_are_set() {
        assert!(pieces('⠀', 10., 20.).is_empty());
        assert_eq!(pieces('⣿', 10., 20.).len(), 8);
        let one = pieces('⠁', 10., 20.);
        let eight = pieces('⢀', 10., 20.);
        assert_eq!(one.len(), 1);
        // Dot 1 is top left, dot 8 bottom right.
        assert!(one[0].x < eight[0].x && one[0].y < eight[0].y);
        for p in pieces('⣿', 9., 19.) {
            assert!(p.x >= 0. && p.x + p.w <= 9. && p.y >= 0. && p.y + p.h <= 19.);
            assert_eq!(p.w, p.w.round(), "whole pixels");
        }
    }

    /// Where each dot of `⣿` starts and how big it is, across and down.
    fn grid(width: f32, height: f32) -> (Vec<f32>, Vec<f32>, f32) {
        let dots = pieces('⣿', width, height);
        let mut xs: Vec<f32> = dots.iter().map(|p| p.x).collect();
        let mut ys: Vec<f32> = dots.iter().map(|p| p.y).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup();
        ys.sort_by(f32::total_cmp);
        ys.dedup();
        (xs, ys, dots[0].w)
    }

    #[test]
    fn braille_spaces_dots_as_ghostty_does() {
        // A 9.5 × 19 pt cell at 2×, as DM Mono 16 px draws: Ghostty's 4 px dots, 5 px
        // apart, not 5 px dots 4 px apart.
        assert_eq!(grid(19., 38.), (vec![3., 12.], vec![3., 12., 21., 30.], 4.));
        // A 10 × 20 cell at 1×: 2 px dots.
        assert_eq!(grid(10., 20.), (vec![1., 6.], vec![1., 6., 11., 16.], 2.));
        // A tiny cell still shows its dots.
        assert_eq!(grid(3., 7.).2, 1.);
    }

    #[test]
    fn blocks_fill_their_share_of_the_cell() {
        assert_eq!(pieces('█', 10., 20.), vec![piece(0., 0., 10., 20.)]);
        assert_eq!(pieces('▄', 10., 20.), vec![piece(0., 10., 10., 10.)]);
        assert_eq!(pieces('▁', 10., 20.), vec![piece(0., 17., 10., 3.)]);
        assert_eq!(pieces('▌', 10., 20.), vec![piece(0., 0., 5., 20.)]);
        assert_eq!(pieces('▐', 10., 20.), vec![piece(5., 0., 5., 20.)]);
        assert_eq!(pieces('▒', 10., 20.)[0].alpha, 0.5);
        assert_eq!(pieces('▚', 10., 20.).len(), 2);
        assert!(Sprite::from_text("a").is_none());
        assert!(Sprite::from_text("⣿").is_some());
        assert!(Sprite::from_text("▓").is_some());
        assert!(Sprite::from_text("│").unwrap().is_box());
        assert!(Sprite::from_text("│\u{301}").is_none());
    }
}
