//! Characters rexia draws itself instead of taking from a font, as Ghostty does: braille
//! (U+2800–28FF) and block elements (U+2580–259F). Fonts often lack braille, and a fallback
//! font's dots don't fit the cell (some draw unset dots hollow, so btop's graphs turn into
//! hatching); blocks drawn from a font leave gaps when the line height isn't the font's.
//! Drawn here, they fill the cell exactly and line up with their neighbors.

/// Whether rexia draws `c` itself.
pub fn is_sprite(c: char) -> bool {
    matches!(c as u32, 0x2800..=0x28FF | 0x2580..=0x259F)
}

/// A filled rectangle within a cell, in pixels from the cell's top-left, with its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

/// The pieces that draw `c` in a cell `width` × `height` device pixels, or nothing when rexia
/// doesn't draw it. Edges land on whole pixels.
pub fn pieces(c: char, width: f32, height: f32) -> Vec<Piece> {
    let code = c as u32;
    match code {
        0x2800..=0x28FF => braille((code - 0x2800) as u8, width.round(), height.round()),
        0x2580..=0x259F => block(code, width.round(), height.round()),
        _ => Vec::new(),
    }
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

/// Square dots on a 2 × 4 grid, sized and spaced as Ghostty sizes them: as big as fits, with
/// even gaps, and the leftover pixels shared out.
fn braille(bits: u8, width: f32, height: f32) -> Vec<Piece> {
    let w = width as i32;
    let h = height as i32;
    let mut dot = (w / 4).min(h / 8).max(1);
    let mut x_gap = w / 4;
    let mut y_gap = h / 8;
    let mut x_margin = x_gap / 2;
    let mut y_margin = y_gap / 2;
    let mut x_left = w - 2 * x_margin - x_gap - 2 * dot;
    let mut y_left = h - 2 * y_margin - 3 * y_gap - 4 * dot;
    // Spare pixels make the dots bigger first, then the gaps, then the margins.
    if x_left >= 2 && y_left >= 4 {
        dot += 1;
        x_left -= 2;
        y_left -= 4;
    }
    if x_left >= 1 {
        x_gap += 1;
        x_left -= 1;
    }
    if y_left >= 3 {
        y_gap += 1;
        y_left -= 3;
    }
    if x_left >= 2 {
        x_margin += 1;
    }
    if y_left >= 2 {
        y_margin += 1;
    }
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

    #[test]
    fn blocks_fill_their_share_of_the_cell() {
        assert_eq!(pieces('█', 10., 20.), vec![piece(0., 0., 10., 20.)]);
        assert_eq!(pieces('▄', 10., 20.), vec![piece(0., 10., 10., 10.)]);
        assert_eq!(pieces('▁', 10., 20.), vec![piece(0., 17., 10., 3.)]);
        assert_eq!(pieces('▌', 10., 20.), vec![piece(0., 0., 5., 20.)]);
        assert_eq!(pieces('▐', 10., 20.), vec![piece(5., 0., 5., 20.)]);
        assert_eq!(pieces('▒', 10., 20.)[0].alpha, 0.5);
        assert_eq!(pieces('▚', 10., 20.).len(), 2);
        assert!(!is_sprite('a') && is_sprite('⣿') && is_sprite('▓'));
    }
}
