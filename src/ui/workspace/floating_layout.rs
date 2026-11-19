use super::Workspace;
use crate::rex::view::{self, Divider, Node, Rect};
use std::collections::HashMap;

/// The number of minimum-width columns a split tree needs side by side.
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

pub(super) type Limits = (f64, f64);
pub(super) const DEFAULT_LIMITS: Limits = (0.05, 0.95);

impl Workspace {
    pub(super) fn nudge_floating_divider(
        &self,
        layer: &view::Layer,
        block: &str,
        direction: &str,
        amount: f64,
        theme: &super::Theme,
    ) -> Option<Divider> {
        let layout = self.fit_floating_layer(layer, theme)?;
        let pane = layout.panes.get(block)?;
        let horizontal = matches!(direction, "left" | "right");
        let (mut divider, limits) = layout
            .dividers
            .into_iter()
            .filter(|(d, _)| {
                let rect = d.rect;
                d.horizontal == horizontal
                    && pane.x >= rect.x - 1e-6
                    && pane.y >= rect.y - 1e-6
                    && pane.x + pane.w <= rect.x + rect.w + 1e-6
                    && pane.y + pane.h <= rect.y + rect.h + 1e-6
            })
            .min_by(|(a, _), (b, _)| (a.rect.w * a.rect.h).total_cmp(&(b.rect.w * b.rect.h)))?;
        let delta = if matches!(direction, "left" | "up") {
            -amount
        } else {
            amount
        };
        divider.ratio = (divider.ratio + delta).clamp(limits.0, limits.1);
        Some(divider)
    }
}

pub(super) struct Layout<'a> {
    pub(super) rect: Rect,
    pub(super) panes: HashMap<&'a str, Rect>,
    pub(super) dividers: Vec<(Divider, Limits)>,
}

impl<'a> Layout<'a> {
    pub(super) fn fit(node: &'a Node, rect: Rect, min_width: f64, drag: Option<&Divider>) -> Self {
        let mut layout = Self {
            rect,
            panes: HashMap::new(),
            dividers: Vec::new(),
        };
        layout.place(node, rect, min_width, drag);
        layout
    }

    fn place(&mut self, node: &'a Node, rect: Rect, min_width: f64, drag: Option<&Divider>) {
        if let Some(block) = node.block_id.as_deref() {
            self.panes.insert(block, rect);
            return;
        }
        let Some(split) = &node.split else { return };
        let horizontal = split.direction != "vertical";
        let (lower, upper) = if horizontal && min_width > 0. && rect.w > 0. {
            let lower = (min_width * columns(&split.before) as f64 / rect.w).min(1.);
            let upper = (1. - min_width * columns(&split.after) as f64 / rect.w).max(lower);
            (lower, upper)
        } else {
            (0., 1.)
        };
        let blocks = view::last_block(&split.before).zip(view::first_block(&split.after));
        let ratio = drag
            .filter(|d| {
                d.horizontal == horizontal && blocks == Some((&d.before_block, &d.after_block))
            })
            .map_or(split.ratio, |d| d.ratio)
            .clamp(lower, upper);
        let limits = (lower.max(DEFAULT_LIMITS.0), upper.min(DEFAULT_LIMITS.1));
        if let Some((before, after)) = blocks
            && limits.1 - limits.0 > 1e-6
        {
            self.dividers.push((
                Divider {
                    split_id: split.split_id.clone(),
                    horizontal,
                    ratio,
                    rect,
                    before_block: before.into(),
                    after_block: after.into(),
                },
                limits,
            ));
        }
        let (mut before, mut after) = (rect, rect);
        if horizontal {
            before.w *= ratio;
            after.x += before.w;
            after.w -= before.w;
        } else {
            before.h *= ratio;
            after.y += before.h;
            after.h -= before.h;
        }
        self.place(&split.before, before, min_width, drag);
        self.place(&split.after, after, min_width, drag);
    }
}
