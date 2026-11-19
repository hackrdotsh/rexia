use gpui::{Bounds, ContentMask, Hsla, PathBuilder, Window, px};

use super::{DeviceCell, piece};

const UP: u8 = 1;
const RIGHT: u8 = 2;
const DOWN: u8 = 4;
const LEFT: u8 = 8;

pub(in crate::ui) enum BoxDrawing {
    Straight { edges: u8, heavy: bool },
    Rounded { x_dir: f32, y_dir: f32 },
}

impl BoxDrawing {
    pub(super) fn from_char(c: char) -> Option<Self> {
        let edges = match c {
            '─' | '━' => LEFT | RIGHT,
            '│' | '┃' => UP | DOWN,
            '┌' | '┏' => DOWN | RIGHT,
            '┐' | '┓' => DOWN | LEFT,
            '└' | '┗' => UP | RIGHT,
            '┘' | '┛' => UP | LEFT,
            '├' | '┣' => UP | RIGHT | DOWN,
            '┤' | '┫' => UP | DOWN | LEFT,
            '┬' | '┳' => RIGHT | DOWN | LEFT,
            '┴' | '┻' => UP | RIGHT | LEFT,
            '┼' | '╋' => UP | RIGHT | DOWN | LEFT,
            '╴' | '╸' => LEFT,
            '╵' | '╹' => UP,
            '╶' | '╺' => RIGHT,
            '╷' | '╻' => DOWN,
            '╭' => {
                return Some(Self::Rounded {
                    x_dir: 1.,
                    y_dir: 1.,
                });
            }
            '╮' => {
                return Some(Self::Rounded {
                    x_dir: -1.,
                    y_dir: 1.,
                });
            }
            '╯' => {
                return Some(Self::Rounded {
                    x_dir: -1.,
                    y_dir: -1.,
                });
            }
            '╰' => {
                return Some(Self::Rounded {
                    x_dir: 1.,
                    y_dir: -1.,
                });
            }
            _ => return None,
        };
        let heavy = "━┃┏┓┗┛┣┫┳┻╋╸╹╺╻".contains(c);
        Some(Self::Straight { edges, heavy })
    }

    pub(super) fn paint(self, cell: DeviceCell, light: f32, fg: Hsla, window: &mut Window) {
        let thickness = if matches!(self, Self::Straight { heavy: true, .. }) {
            2. * light
        } else {
            light
        };
        let thickness = thickness.min(cell.width).min(cell.height);
        let left = ((cell.width - thickness) / 2.).floor();
        let top = ((cell.height - thickness) / 2.).floor();
        match self {
            Self::Straight { edges, .. } => {
                let right = left + thickness;
                let bottom = top + thickness;
                let has = |side| edges & side != 0;
                // Split the arms from the center so faint strokes never blend twice.
                let x0 = if has(LEFT) { 0. } else { left };
                let x1 = if has(RIGHT) { cell.width } else { right };
                cell.paint(piece(x0, top, x1 - x0, thickness), fg, window);
                if has(UP) {
                    cell.paint(piece(left, 0., thickness, top), fg, window);
                }
                if has(DOWN) {
                    cell.paint(
                        piece(left, bottom, thickness, cell.height - bottom),
                        fg,
                        window,
                    );
                }
            }
            Self::Rounded { x_dir, y_dir } => {
                let x = left + thickness / 2.;
                let y = top + thickness / 2.;
                let x_edge = if x_dir > 0. { cell.width } else { 0. };
                let y_edge = if y_dir > 0. { cell.height } else { 0. };
                let at = |x, y| cell.point(x, y);
                // Ghostty's rounded corner uses a cubic with control points one quarter
                // radius from the center, not a circular arc.
                let radius = cell.width.min(cell.height) / 2.;
                let cx = radius * x_dir;
                let cy = radius * y_dir;
                let mut path = PathBuilder::stroke(px(thickness / cell.scale));
                path.move_to(at(x, y_edge));
                path.line_to(at(x, y + cy));
                path.cubic_bezier_to(at(x + cx, y), at(x, y + cy / 4.), at(x + cx / 4., y));
                path.line_to(at(x_edge, y));
                if let Ok(path) = path.build() {
                    let bounds = Bounds::from_corners(
                        cell.point(0., 0.),
                        cell.point(cell.width, cell.height),
                    );
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        window.paint_path(path, fg);
                    });
                }
            }
        }
    }
}
