use crate::rex::view::{Node, Rect};
use std::collections::HashMap;

/// The number of minimum-width columns this split tree needs side by side.
pub(super) fn columns(node: &Node) -> usize {
    let Some(split) = &node.split else {
        return 1;
    };
    let before = columns(&split.before);
    let after = columns(&split.after);
    if split.direction == "vertical" {
        before.max(after)
    } else {
        before + after
    }
}

/// Clamp the displayed split widths without changing the server's layout.
pub(super) fn horizontal_bounds<'a>(
    node: &'a Node,
    rect: Rect,
    min_width: f64,
    bounds: &mut HashMap<&'a str, (f64, f64)>,
) {
    if let Some(block) = node.block_id.as_deref() {
        bounds.insert(block, (rect.x, rect.w));
    } else if let Some(split) = &node.split {
        let (mut before, mut after) = (rect, rect);
        if split.direction != "vertical" {
            let lower = (min_width * columns(&split.before) as f64).min(rect.w);
            let upper = rect.w - min_width * columns(&split.after) as f64;
            // The caller budgets for both sides. Account for rounding at an exact fit.
            before.w = (rect.w * split.ratio).clamp(lower, upper.max(lower));
            after.x += before.w;
            after.w -= before.w;
        }
        horizontal_bounds(&split.before, before, min_width, bounds);
        horizontal_bounds(&split.after, after, min_width, bounds);
    }
}
