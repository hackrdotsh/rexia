//! Cursor shaders: a trail drawn behind the cursor when it jumps, in one of several
//! effects. GPUI has no custom GPU shaders, so each effect is a filled shape between where
//! the cursor was and where it is, with its own color, taper and fade.
use crate::settings::CursorShader;
use gpui::{
    Background, Bounds, Hsla, PathBuilder, Pixels, Point, Window, linear_color_stop,
    linear_gradient, point, px,
};
use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

/// A cursor position in cells, and how many cells wide it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSpot {
    pub col: u16,
    pub row: u16,
    pub width: u16,
}

impl CellSpot {
    /// How far the cursor jumped, in cells: moving one cell while typing doesn't count.
    pub fn distance(self, other: CellSpot) -> u32 {
        u32::from(self.col.abs_diff(other.col)) + u32::from(self.row.abs_diff(other.row))
    }
}

/// One trail: from where the cursor was to where it is, starting at `started`.
#[derive(Clone, Copy, Debug)]
pub struct Trail {
    pub from: CellSpot,
    pub to: CellSpot,
    pub started: Instant,
    pub shader: CursorShader,
}

/// Moves of this many cells or more draw a trail; typing moves one at a time.
pub const MIN_JUMP: u32 = 2;

/// Progress held fixed for pictures, in thousandths; 0 lets trails run.
static HELD: AtomicU32 = AtomicU32::new(0);

/// Hold every trail at `progress` (0 to 1), so screenshots can show one; `None` lets
/// them run again.
pub fn hold_progress(progress: Option<f32>) {
    let value = progress.map_or(0, |p| (p.clamp(0.001, 1.) * 1000.) as u32);
    HELD.store(value, Ordering::Relaxed);
}

impl CursorShader {
    pub const ALL: [CursorShader; 8] = [
        CursorShader::Warp,
        CursorShader::Sweep,
        CursorShader::Comet,
        CursorShader::Blaze,
        CursorShader::Tapered,
        CursorShader::Rainbow,
        CursorShader::Gradient,
        CursorShader::Frozen,
    ];

    pub fn title(self) -> &'static str {
        match self {
            CursorShader::Warp => "Warp",
            CursorShader::Sweep => "Sweep",
            CursorShader::Comet => "Comet",
            CursorShader::Blaze => "Blaze",
            CursorShader::Tapered => "Tapered",
            CursorShader::Rainbow => "Rainbow",
            CursorShader::Gradient => "Gradient",
            CursorShader::Frozen => "Frozen",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            CursorShader::Warp => "The cursor stretches to where it's going, then snaps after it",
            CursorShader::Sweep => "A band between both places that fades where it is",
            CursorShader::Comet => "A tail narrowing behind a glowing head",
            CursorShader::Blaze => "A fiery tail, yellow at the cursor and red behind",
            CursorShader::Tapered => "A tail that narrows to a point",
            CursorShader::Rainbow => "A tail that runs through the colors",
            CursorShader::Gradient => "The cursor's color fading into the accent",
            CursorShader::Frozen => "An icy band that lingers and melts",
        }
    }

    /// How long the trail lasts.
    pub fn duration(self) -> Duration {
        Duration::from_millis(match self {
            CursorShader::Warp | CursorShader::Tapered => 160,
            CursorShader::Sweep | CursorShader::Gradient => 220,
            CursorShader::Comet | CursorShader::Blaze | CursorShader::Rainbow => 280,
            CursorShader::Frozen => 420,
        })
    }

    /// Whether the tail narrows to a point rather than keeping the cursor's height.
    fn tapers(self) -> bool {
        matches!(
            self,
            CursorShader::Comet
                | CursorShader::Blaze
                | CursorShader::Tapered
                | CursorShader::Rainbow
        )
    }

    /// Whether the tail stays where the cursor was while the band fades, rather than
    /// catching up with the cursor.
    fn stays(self) -> bool {
        matches!(self, CursorShader::Sweep | CursorShader::Frozen)
    }
}

impl Trail {
    /// How far along the trail is, from 0 to 1; `None` once it's over.
    pub fn progress(&self, now: Instant) -> Option<f32> {
        let held = HELD.load(Ordering::Relaxed);
        if held > 0 {
            return Some(held as f32 / 1000.);
        }
        let t =
            now.duration_since(self.started).as_secs_f32() / self.shader.duration().as_secs_f32();
        (t < 1.).then_some(t)
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

fn lerp(a: Point<Pixels>, b: Point<Pixels>, t: f32) -> Point<Pixels> {
    point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn corners(rect: Bounds<Pixels>) -> [Point<Pixels>; 4] {
    [
        rect.origin,
        point(rect.origin.x + rect.size.width, rect.origin.y),
        rect.origin + point(rect.size.width, rect.size.height),
        point(rect.origin.x, rect.origin.y + rect.size.height),
    ]
}

/// The convex hull of `points`, counterclockwise (Andrew's monotone chain).
pub fn hull(mut points: Vec<Point<Pixels>>) -> Vec<Point<Pixels>> {
    points.sort_by(|a, b| {
        f32::from(a.x)
            .total_cmp(&f32::from(b.x))
            .then(f32::from(a.y).total_cmp(&f32::from(b.y)))
    });
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: Point<Pixels>, a: Point<Pixels>, b: Point<Pixels>| {
        f32::from(a.x - o.x) * f32::from(b.y - o.y) - f32::from(a.y - o.y) * f32::from(b.x - o.x)
    };
    let mut lower: Vec<Point<Pixels>> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0. {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Point<Pixels>> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0. {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The trail's outline at progress `t`, between the cursor's old and new cells.
pub fn trail_shape(
    shader: CursorShader,
    from: Bounds<Pixels>,
    to: Bounds<Pixels>,
    t: f32,
) -> Vec<Point<Pixels>> {
    let eased = ease_out(t);
    let mut points = corners(to).to_vec();
    if shader.tapers() {
        // The tail's point runs after the cursor.
        points.push(lerp(from.center(), to.center(), eased));
    } else {
        let tail = if shader.stays() {
            from
        } else {
            Bounds::new(lerp(from.origin, to.origin, eased), from.size)
        };
        points.extend(corners(tail));
    }
    hull(points)
}

/// What the trail is filled with at progress `t`: a solid color or a gradient running
/// from the tail toward the cursor.
pub fn trail_fill(
    shader: CursorShader,
    from: Bounds<Pixels>,
    to: Bounds<Pixels>,
    t: f32,
    cursor: Hsla,
    accent: Hsla,
) -> Background {
    let fade = 1. - t;
    let along = {
        // CSS angles: 0 points up, 90 right; the gradient's end is at the cursor.
        let dx = f32::from(to.center().x - from.center().x);
        let dy = f32::from(to.center().y - from.center().y);
        dx.atan2(-dy).to_degrees()
    };
    let gradient = |tail: Hsla, head: Hsla| {
        linear_gradient(
            along,
            linear_color_stop(tail, 0.),
            linear_color_stop(head, 1.),
        )
    };
    match shader {
        CursorShader::Warp | CursorShader::Tapered => cursor.opacity(0.75 * fade).into(),
        CursorShader::Sweep => cursor.opacity(0.45 * fade).into(),
        CursorShader::Comet => gradient(cursor.opacity(0.), cursor.opacity(0.85 * fade)),
        CursorShader::Blaze => gradient(
            gpui::hsla(0.0, 0.95, 0.45, 0.),
            gpui::hsla(0.12, 1., 0.6, 0.95 * fade),
        ),
        CursorShader::Rainbow => {
            let hue = (t * 1.5) % 1.;
            gradient(
                gpui::hsla((hue + 0.5) % 1., 0.9, 0.6, 0.15 * fade),
                gpui::hsla(hue, 0.9, 0.6, 0.9 * fade),
            )
        }
        CursorShader::Gradient => gradient(accent.opacity(0.2 * fade), cursor.opacity(0.85 * fade)),
        CursorShader::Frozen => gradient(
            gpui::hsla(0.55, 0.9, 0.85, 0.15 * fade),
            gpui::hsla(0.53, 0.85, 0.65, 0.7 * fade),
        ),
    }
}

/// A glow around the cursor's new place, for the effects that have one.
fn glow(
    shader: CursorShader,
    to: Bounds<Pixels>,
    t: f32,
    cursor: Hsla,
) -> Option<(Bounds<Pixels>, Hsla)> {
    let color = match shader {
        CursorShader::Comet => cursor,
        CursorShader::Blaze => gpui::hsla(0.08, 1., 0.55, 1.),
        _ => return None,
    };
    let grow = px(6.) * (1. - t);
    let area = Bounds::new(
        to.origin - point(grow, grow),
        to.size + gpui::size(grow * 2., grow * 2.),
    );
    Some((area, color.opacity(0.35 * (1. - t))))
}

/// Paint `trail` at progress `t`, its cells placed by `cell_bounds`.
pub fn paint_trail(
    window: &mut Window,
    trail: &Trail,
    t: f32,
    cell_bounds: impl Fn(CellSpot) -> Bounds<Pixels>,
    cursor: Hsla,
    accent: Hsla,
) {
    let (from, to) = (cell_bounds(trail.from), cell_bounds(trail.to));
    let shape = trail_shape(trail.shader, from, to, t);
    if shape.len() >= 3 {
        let mut path = PathBuilder::fill();
        path.move_to(shape[0]);
        for &p in &shape[1..] {
            path.line_to(p);
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, trail_fill(trail.shader, from, to, t, cursor, accent));
        }
    }
    if let Some((area, color)) = glow(trail.shader, to, t, cursor) {
        window.paint_quad(gpui::fill(area, color).corner_radii(px(4.)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn cell(x: f32, y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(10.), px(20.)))
    }

    #[test]
    fn hulls_wrap_both_cells() {
        let shape = hull([corners(cell(0., 0.)), corners(cell(50., 40.))].concat());
        // A diagonal jump: two corners of each cell are inside the band.
        assert_eq!(shape.len(), 6);
        assert!(shape.contains(&point(px(0.), px(0.))));
        assert!(shape.contains(&point(px(60.), px(60.))));
    }

    #[test]
    fn shapes_follow_their_effect() {
        let (from, to) = (cell(0., 0.), cell(100., 0.));
        // Warp: the tail catches up, so by the end only the cursor's cell is left.
        let end = trail_shape(CursorShader::Warp, from, to, 1.);
        assert!(end.iter().all(|p| p.x >= px(100.)), "{end:?}");
        // Sweep stays where it started while it fades.
        let sweep = trail_shape(CursorShader::Sweep, from, to, 0.9);
        assert!(sweep.contains(&point(px(0.), px(0.))));
        // Tapered: the tail is a point, so the outline is the cell plus that point.
        let tapered = trail_shape(CursorShader::Tapered, from, to, 0.);
        assert_eq!(tapered.len(), 5);
        assert!(tapered.contains(&point(px(5.), px(10.))));
    }

    #[test]
    fn jumps_count_and_trails_end() {
        let a = CellSpot {
            col: 2,
            row: 1,
            width: 1,
        };
        let b = CellSpot {
            col: 3,
            row: 1,
            width: 1,
        };
        let c = CellSpot {
            col: 0,
            row: 5,
            width: 1,
        };
        assert!(a.distance(b) < MIN_JUMP, "typing");
        assert!(a.distance(c) >= MIN_JUMP);
        let trail = Trail {
            from: a,
            to: c,
            started: Instant::now(),
            shader: CursorShader::Frozen,
        };
        assert!(trail.progress(trail.started).is_some());
        assert_eq!(trail.progress(trail.started + Duration::from_secs(1)), None);
        assert_eq!(CursorShader::ALL.len(), 8);
    }
}
