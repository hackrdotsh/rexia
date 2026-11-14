//! A session's view as `session.view` describes it, and the pane geometry derived from it.
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct View {
    pub revision: u64,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub active_window_id: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Window {
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused_block_id: Option<String>,
    #[serde(default)]
    pub zoomed_block_id: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Layer {
    #[serde(default)]
    pub layer_id: Option<String>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub bounds: Vec<f64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub layout: Option<Node>,
    #[serde(default, deserialize_with = "nullable")]
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Node {
    #[serde(default)]
    pub rect: Vec<f64>,
    #[serde(default)]
    pub block_id: Option<String>,
    #[serde(default)]
    pub split: Option<Box<Split>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Split {
    #[serde(default)]
    pub split_id: Option<String>,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub ratio: f64,
    pub before: Node,
    pub after: Node,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Block {
    #[serde(default)]
    pub block_id: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub creator_name: String,
    #[serde(default)]
    pub flavor: String,
    #[serde(default)]
    pub host_id: String,
    #[serde(default)]
    pub owner: Option<String>,
}

fn nullable<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// A rectangle as [x, y, w, h] fractions of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const FULL: Rect = Rect {
        x: 0.,
        y: 0.,
        w: 1.,
        h: 1.,
    };

    fn from_slice(values: &[f64]) -> Option<Self> {
        match values {
            [x, y, w, h] => Some(Self {
                x: *x,
                y: *y,
                w: *w,
                h: *h,
            }),
            _ => None,
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
}

/// Where one pane sits in its window.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub block_id: String,
    pub rect: Rect,
    /// The floating layer that holds it, if any.
    pub floating: Option<String>,
}

/// The draggable border between a split's two sides.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub split_id: Option<String>,
    /// True for a border between left and right.
    pub horizontal: bool,
    pub ratio: f64,
    /// The split's whole area.
    pub rect: Rect,
    /// A block on each side, for `session.resize_pane`.
    pub before_block: String,
    pub after_block: String,
}

impl Window {
    pub fn id(&self) -> &str {
        self.window_id.as_deref().unwrap_or_default()
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.layers.iter().flat_map(|layer| &layer.blocks)
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks().find(|b| b.block_id.as_deref() == Some(id))
    }

    /// Visible panes, back to front: the tiled layout, then each floating layer that
    /// isn't minimized. A zoomed block covers the whole window.
    pub fn panes(&self) -> Vec<Pane> {
        if let Some(zoomed) = &self.zoomed_block_id {
            return vec![Pane {
                block_id: zoomed.clone(),
                rect: Rect::FULL,
                floating: None,
            }];
        }
        let mut panes = Vec::new();
        for layer in &self.layers {
            if layer.state.as_deref() == Some("minimized") {
                continue;
            }
            let floating =
                (layer.kind != "tiled").then(|| layer.layer_id.clone().unwrap_or_default());
            if let Some(node) = &layer.layout {
                let bounds = Rect::from_slice(&layer.bounds).unwrap_or(Rect::FULL);
                collect_panes(node, bounds, &floating, &mut panes);
            }
        }
        panes
    }

    /// Dividers of the tiled layout (floating layers resize as a whole).
    pub fn dividers(&self) -> Vec<Divider> {
        if self.zoomed_block_id.is_some() {
            return Vec::new();
        }
        let mut dividers = Vec::new();
        for layer in self.layers.iter().filter(|l| l.kind == "tiled") {
            if let Some(node) = &layer.layout {
                collect_dividers(node, &mut dividers);
            }
        }
        dividers
    }

    /// Floating layers that are minimized, for the restore control.
    pub fn minimized_layers(&self) -> Vec<&Layer> {
        self.layers
            .iter()
            .filter(|l| l.state.as_deref() == Some("minimized"))
            .collect()
    }

    pub fn layer_of(&self, block_id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| {
            l.blocks
                .iter()
                .any(|b| b.block_id.as_deref() == Some(block_id))
        })
    }
}

impl View {
    pub fn active_window(&self) -> Option<&Window> {
        let active = self.active_window_id.as_deref();
        self.windows
            .iter()
            .find(|w| w.window_id.as_deref() == active)
            .or(self.windows.first())
    }

    pub fn window(&self, id: &str) -> Option<&Window> {
        self.windows
            .iter()
            .find(|w| w.window_id.as_deref() == Some(id))
    }

    pub fn window_of(&self, block_id: &str) -> Option<&Window> {
        self.windows.iter().find(|w| w.block(block_id).is_some())
    }

    pub fn terminal_blocks(&self) -> impl Iterator<Item = &Block> {
        self.windows
            .iter()
            .flat_map(Window::blocks)
            .filter(|b| b.creator_name == super::data::TERMINAL_CREATOR)
    }
}

fn node_rect(node: &Node, fallback: Rect) -> Rect {
    Rect::from_slice(&node.rect).unwrap_or(fallback)
}

fn collect_panes(node: &Node, fallback: Rect, floating: &Option<String>, out: &mut Vec<Pane>) {
    let rect = node_rect(node, fallback);
    if let Some(block) = &node.block_id {
        out.push(Pane {
            block_id: block.clone(),
            rect,
            floating: floating.clone(),
        });
    } else if let Some(split) = &node.split {
        let (before, after) = split_rects(split, rect);
        collect_panes(&split.before, before, floating, out);
        collect_panes(&split.after, after, floating, out);
    }
}

fn split_rects(split: &Split, rect: Rect) -> (Rect, Rect) {
    let ratio = split.ratio.clamp(0., 1.);
    if split.direction == "vertical" {
        (
            Rect {
                h: rect.h * ratio,
                ..rect
            },
            Rect {
                y: rect.y + rect.h * ratio,
                h: rect.h * (1. - ratio),
                ..rect
            },
        )
    } else {
        (
            Rect {
                w: rect.w * ratio,
                ..rect
            },
            Rect {
                x: rect.x + rect.w * ratio,
                w: rect.w * (1. - ratio),
                ..rect
            },
        )
    }
}

pub fn first_block(node: &Node) -> Option<&str> {
    node.block_id
        .as_deref()
        .or_else(|| first_block(&node.split.as_ref()?.before))
}

pub fn last_block(node: &Node) -> Option<&str> {
    node.block_id
        .as_deref()
        .or_else(|| last_block(&node.split.as_ref()?.after))
}

fn collect_dividers(node: &Node, out: &mut Vec<Divider>) {
    let Some(split) = &node.split else {
        return;
    };
    // The blocks nearest the border name it unambiguously for resize_pane.
    if let (Some(before), Some(after)) = (last_block(&split.before), first_block(&split.after)) {
        out.push(Divider {
            split_id: split.split_id.clone(),
            horizontal: split.direction != "vertical",
            ratio: split.ratio,
            rect: node_rect(node, Rect::FULL),
            before_block: before.into(),
            after_block: after.into(),
        });
    }
    collect_dividers(&split.before, out);
    collect_dividers(&split.after, out);
}

/// A split's new ratio for `session.resize_pane`, named by the blocks beside its border.
#[derive(Clone, Debug, PartialEq)]
pub struct Ratio {
    pub before_block: String,
    pub after_block: String,
    pub horizontal: bool,
    pub ratio: f64,
}

/// Ratios that give every pane in a run of same-direction splits an equal share.
pub fn balanced(window: &Window) -> Vec<Ratio> {
    fn weight(node: &Node, horizontal: bool) -> f64 {
        match &node.split {
            None => 1.,
            Some(split) if (split.direction != "vertical") == horizontal => {
                weight(&split.before, horizontal) + weight(&split.after, horizontal)
            }
            Some(split) => weight(&split.before, horizontal).max(weight(&split.after, horizontal)),
        }
    }
    fn walk(node: &Node, out: &mut Vec<Ratio>) {
        let Some(split) = &node.split else { return };
        let horizontal = split.direction != "vertical";
        if let (Some(before), Some(after)) = (last_block(&split.before), first_block(&split.after))
        {
            let a = weight(&split.before, horizontal);
            let b = weight(&split.after, horizontal);
            out.push(Ratio {
                before_block: before.into(),
                after_block: after.into(),
                horizontal,
                ratio: a / (a + b),
            });
        }
        walk(&split.before, out);
        walk(&split.after, out);
    }
    let mut out = Vec::new();
    for layer in window.layers.iter().filter(|l| l.kind == "tiled") {
        if let Some(node) = &layer.layout {
            walk(node, &mut out);
        }
    }
    out
}

/// The nearest pane in a direction from `from`, among panes that overlap it across that axis.
pub fn neighbor<'p>(panes: &'p [Pane], from: &str, direction: &str) -> Option<&'p Pane> {
    let origin = panes.iter().find(|p| p.block_id == from)?.rect;
    let (cx, cy) = origin.center();
    let overlaps = |a0: f64, a1: f64, b0: f64, b1: f64| a0 < b1 - 1e-6 && b0 < a1 - 1e-6;
    panes
        .iter()
        .filter(|p| p.block_id != from && p.floating.is_none())
        .filter(|p| {
            let r = p.rect;
            let (px, py) = r.center();
            match direction {
                "left" => px < cx && overlaps(r.y, r.y + r.h, origin.y, origin.y + origin.h),
                "right" => px > cx && overlaps(r.y, r.y + r.h, origin.y, origin.y + origin.h),
                "up" => py < cy && overlaps(r.x, r.x + r.w, origin.x, origin.x + origin.w),
                "down" => py > cy && overlaps(r.x, r.x + r.w, origin.x, origin.x + origin.w),
                _ => false,
            }
        })
        .min_by(|a, b| {
            let d = |p: &Pane| {
                let (px, py) = p.rect.center();
                (px - cx).powi(2) + (py - cy).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
}

/// The innermost divider beside `block` that moves in `direction`, with the ratio after
/// moving it by `amount` of its split.
pub fn nudge(window: &Window, block: &str, direction: &str, amount: f64) -> Option<Divider> {
    let horizontal = matches!(direction, "left" | "right");
    let pane = window
        .panes()
        .into_iter()
        .find(|p| p.block_id == block)?
        .rect;
    let inside = |outer: &Rect| {
        pane.x >= outer.x - 1e-6
            && pane.y >= outer.y - 1e-6
            && pane.x + pane.w <= outer.x + outer.w + 1e-6
            && pane.y + pane.h <= outer.y + outer.h + 1e-6
    };
    let mut divider = window
        .dividers()
        .into_iter()
        .filter(|d| d.horizontal == horizontal && inside(&d.rect))
        .min_by(|a, b| (a.rect.w * a.rect.h).total_cmp(&(b.rect.w * b.rect.h)))?;
    let delta = if matches!(direction, "left" | "up") {
        -amount
    } else {
        amount
    };
    divider.ratio = (divider.ratio + delta).clamp(0.05, 0.95);
    Some(divider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view() -> View {
        serde_json::from_value(json!({
            "revision": 3, "session_id": "session:1", "label": "work",
            "active_window_id": "window:1",
            "windows": [{
                "window_id": "window:1", "label": "Shell", "focused_block_id": "b",
                "layers": [
                    {"layer_id": "layer:1", "kind": "tiled", "bounds": [0,0,1,1],
                     "layout": {"rect": [0,0,1,1], "split": {"split_id": "split:1",
                        "direction": "horizontal", "ratio": 0.6,
                        "before": {"rect": [0,0,0.6,1], "block_id": "a"},
                        "after": {"rect": [0.6,0,0.4,1], "split": {"split_id": "split:2",
                            "direction": "vertical", "ratio": 0.5,
                            "before": {"rect": [0.6,0,0.4,0.5], "block_id": "b"},
                            "after": {"rect": [0.6,0.5,0.4,0.5], "block_id": "c"}}}}},
                     "blocks": [
                        {"block_id": "a", "creator_name": "com.superlogical.terminal"},
                        {"block_id": "b", "creator_name": "com.superlogical.terminal"},
                        {"block_id": "c", "creator_name": "com.superlogical.terminal"}]},
                    {"layer_id": "layer:2", "kind": "floating", "bounds": [0.2,0.2,0.5,0.5],
                     "layout": {"rect": [0.2,0.2,0.5,0.5], "block_id": "f"},
                     "blocks": [{"block_id": "f", "creator_name": "com.superlogical.terminal"}]},
                    {"layer_id": "layer:3", "kind": "floating", "bounds": [0,0,0.3,0.3],
                     "state": "minimized",
                     "layout": {"rect": [0,0,0.3,0.3], "block_id": "m"},
                     "blocks": [{"block_id": "m"}]}
                ]}]
        }))
        .unwrap()
    }

    #[test]
    fn panes_follow_the_server_layout_back_to_front() {
        let view = view();
        let window = view.active_window().unwrap();
        let panes = window.panes();
        let ids: Vec<_> = panes.iter().map(|p| p.block_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "f"]);
        assert_eq!(
            panes[2].rect,
            Rect {
                x: 0.6,
                y: 0.5,
                w: 0.4,
                h: 0.5
            }
        );
        assert_eq!(panes[3].floating.as_deref(), Some("layer:2"));
        assert_eq!(window.minimized_layers().len(), 1);
        assert_eq!(view.terminal_blocks().count(), 4);
    }

    #[test]
    fn dividers_name_the_blocks_beside_each_border() {
        let view = view();
        let dividers = view.active_window().unwrap().dividers();
        assert_eq!(dividers.len(), 2);
        assert!(dividers[0].horizontal);
        assert_eq!(
            (
                dividers[0].before_block.as_str(),
                dividers[0].after_block.as_str()
            ),
            ("a", "b")
        );
        assert!(!dividers[1].horizontal);
        assert_eq!(
            (
                dividers[1].before_block.as_str(),
                dividers[1].after_block.as_str()
            ),
            ("b", "c")
        );
    }

    #[test]
    fn zoom_covers_the_window_and_hides_dividers() {
        let mut view = view();
        view.windows[0].zoomed_block_id = Some("c".into());
        let window = view.active_window().unwrap();
        assert_eq!(
            window.panes(),
            vec![Pane {
                block_id: "c".into(),
                rect: Rect::FULL,
                floating: None
            }]
        );
        assert!(window.dividers().is_empty());
    }

    #[test]
    fn balance_neighbors_and_nudges() {
        let view = view();
        let window = view.active_window().unwrap();
        let ratios = balanced(window);
        // a | (b / c): the outer split weighs one column against one column.
        assert_eq!(ratios[0].ratio, 0.5);
        assert_eq!(ratios[1].ratio, 0.5);
        let panes = window.panes();
        assert_eq!(neighbor(&panes, "a", "right").unwrap().block_id, "b");
        assert_eq!(neighbor(&panes, "c", "up").unwrap().block_id, "b");
        assert_eq!(neighbor(&panes, "c", "left").unwrap().block_id, "a");
        assert!(neighbor(&panes, "a", "left").is_none());
        let moved = nudge(window, "b", "down", 0.1).unwrap();
        assert_eq!((moved.before_block.as_str(), moved.ratio), ("b", 0.6));
        let wider = nudge(window, "a", "right", 0.05).unwrap();
        assert!((wider.ratio - 0.65).abs() < 1e-9);
    }

    #[test]
    fn null_lists_read_as_empty() {
        let view: View =
            serde_json::from_value(json!({"revision": 1, "session_id": "s", "windows": null}))
                .unwrap();
        assert!(view.windows.is_empty());
        assert!(view.active_window().is_none());
    }
}
