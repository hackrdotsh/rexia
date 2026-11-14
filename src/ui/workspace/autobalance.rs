//! Autobalance splits: when a pane opens or closes in the tab on screen, its splits even
//! out. A divider someone drags or moves from the keyboard stays where they put it until
//! Balance Splits.
use super::Workspace;
use crate::rex::view;
use serde_json::json;

/// A divider dropped this close to the middle of its split goes to the middle.
const SNAP: f64 = 0.03;

/// The ratio a dragged divider ends at: the middle when it's near it.
pub fn snapped(ratio: f64) -> f64 {
    if (ratio - 0.5).abs() < SNAP {
        0.5
    } else {
        ratio
    }
}

impl Workspace {
    /// Balance the tab on screen if its panes changed since rexia last looked.
    pub(super) fn autobalance(&mut self) {
        let model = self.model();
        let Some(session) = model.selected.clone() else {
            return;
        };
        let Some(window) = model.view().and_then(view::View::active_window) else {
            return;
        };
        let Some(id) = window.window_id.clone() else {
            return;
        };
        let blocks = view::tiled_blocks(window);
        let before = self.tiled_seen.insert(id, blocks.clone());
        let changed = before.is_some_and(|before| before != blocks);
        if !changed
            || !self.settings.interface.autobalance_splits
            || window.zoomed_block_id.is_some()
        {
            return;
        }
        for ratio in view::rebalanced(window, &self.pinned_splits) {
            self.call(
                "session.resize_pane",
                json!({"session_id": session,
                    "direction": if ratio.horizontal { "horizontal" } else { "vertical" },
                    "before_block_id": ratio.before_block, "after_block_id": ratio.after_block,
                    "ratio": ratio.ratio}),
            );
        }
    }

    /// Keep a divider where someone put it.
    pub(super) fn pin_split(&mut self, split: Option<String>) {
        self.pinned_splits.extend(split);
    }

    /// Balance Splits lets the tab's dividers move with autobalance again.
    pub(super) fn unpin_splits(&mut self, window: &view::Window) {
        for id in view::split_ids(window) {
            self.pinned_splits.remove(&id);
        }
    }

    /// The splits kept where someone put them, for tests.
    pub fn pinned_splits(&self) -> Vec<String> {
        let mut out: Vec<String> = self.pinned_splits.iter().cloned().collect();
        out.sort();
        out
    }
}
