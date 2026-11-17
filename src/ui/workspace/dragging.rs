//! Dragging tabs and panes, as Rex for Mac does: a tab dropped between others moves there;
//! a pane, picked up by its header or with Ctrl held anywhere over it, drops beside another
//! pane, onto another tab to move there, or between tabs to become a tab of its own. A drag
//! starts once the pointer moves a few pixels, shows where it will land, and Escape cancels.
use super::Workspace;
use crate::{
    rex::{host::shell_layout_for, view},
    ui::{theme::Theme, titles},
};
use gpui::{
    AnyElement, Bounds, Context, Pixels, Point, deferred, div, point, prelude::*, px, size,
};
use serde_json::json;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// How far the pointer moves before a press becomes a drag.
const THRESHOLD: f32 = 4.;
pub(super) const SESSION_ROW_GAP: Pixels = px(2.);

/// A temporary order based on the unshifted group bounds at the start of the drag.
pub(super) struct SessionPreview {
    host: usize,
    grabbed: String,
    grab_offset: Option<Pixels>,
    spots: Vec<(String, Bounds<Pixels>)>,
    order: Vec<String>,
    offsets: HashMap<String, Pixels>,
    started: Instant,
    pending: bool,
}

pub(super) struct SessionPreviewRows<'a> {
    pub order: &'a [String],
    preview: &'a SessionPreview,
    remaining: f32,
    pub animating: bool,
}

impl SessionPreviewRows<'_> {
    pub fn placement(&self, id: &str, reduce_motion: bool) -> (Pixels, bool) {
        if id == self.preview.grabbed
            && let Some(offset) = self.preview.grab_offset
        {
            return (offset, true);
        }
        let remaining = if reduce_motion { 0. } else { self.remaining };
        (
            self.preview.offsets.get(id).copied().unwrap_or_default() * remaining,
            false,
        )
    }
}

impl SessionPreview {
    fn positions(&self, order: &[String]) -> HashMap<String, Pixels> {
        let mut y = Pixels::ZERO;
        order
            .iter()
            .map(|id| {
                let height = self
                    .spots
                    .iter()
                    .find(|(key, _)| key == id)
                    .unwrap()
                    .1
                    .size
                    .height;
                let position = y;
                y += height + SESSION_ROW_GAP;
                (id.clone(), position)
            })
            .collect()
    }

    fn remaining(&self, style: crate::settings::AnimationStyle) -> f32 {
        style.duration().map_or(0., |duration| {
            let progress = (self.started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.);
            1. - super::motion::easing(style)(progress)
        })
    }

    fn retarget(&mut self, order: Vec<String>, style: crate::settings::AnimationStyle) {
        if self.order == order {
            return;
        }
        let previous = self.positions(&self.order);
        let next = self.positions(&order);
        let remaining = self.remaining(style);
        self.offsets = order
            .iter()
            .map(|id| {
                let offset = if id == &self.grabbed
                    && let Some(offset) = self.grab_offset
                {
                    offset
                } else {
                    self.offsets.get(id).copied().unwrap_or_default() * remaining
                };
                (id.clone(), previous[id] + offset - next[id])
            })
            .collect();
        self.order = order;
        self.started = Instant::now();
    }

    fn settle(&mut self, style: crate::settings::AnimationStyle) {
        let remaining = self.remaining(style);
        for offset in self.offsets.values_mut() {
            *offset *= remaining;
        }
        if let Some(offset) = self.grab_offset.take() {
            self.offsets.insert(self.grabbed.clone(), offset);
        }
        self.pending = true;
        self.started = Instant::now();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
    /// A tab, by its window ID.
    Tab(String),
    /// A pane, by its block ID.
    Pane(String),
    /// A session in the sidebar: its host and ID.
    Session(usize, String),
}

pub(super) struct Moving {
    pub grab: Grab,
    pub session: String,
    start: Point<Pixels>,
    pub at: Point<Pixels>,
    /// Past the threshold: a drag rather than a click.
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Where a drag would land.
#[derive(Clone, Debug, PartialEq)]
pub enum Landing {
    /// The tab goes just before this one, or last.
    TabBefore(Option<String>),
    /// The pane moves into this tab.
    IntoTab(String),
    /// The pane becomes a new tab, just before this one, or last.
    NewTab(Option<String>),
    /// The pane goes beside this one, on this side.
    Beside { anchor: String, side: Side },
    /// The session goes just before this one of its host's, or last.
    SessionBefore(Option<String>),
}

/// A tab on screen and where it's drawn, in order.
struct TabSpot {
    id: String,
    bounds: Bounds<Pixels>,
    /// Tabs listed down the sidebar rather than across the top.
    vertical: bool,
}

impl Workspace {
    /// A press on a tab or pane that may become a drag.
    pub(super) fn press_for_drag(&mut self, grab: Grab, at: Point<Pixels>) {
        let Some(session) = self.model().selected else {
            return;
        };
        self.moving = Some(Moving {
            grab,
            session,
            start: at,
            at,
            active: false,
        });
    }

    /// A press on a session row that may become a drag to reorder it.
    pub(super) fn press_session_for_drag(&mut self, host: usize, id: String, at: Point<Pixels>) {
        if self
            .session_preview
            .as_ref()
            .is_some_and(|preview| preview.pending)
        {
            return;
        }
        self.moving = Some(Moving {
            grab: Grab::Session(host, id.clone()),
            session: id,
            start: at,
            at,
            active: false,
        });
    }

    /// The session rows of `host` where they're drawn, in order.
    fn session_spots(&self, host: usize) -> Vec<(String, Bounds<Pixels>)> {
        self.session_order(host)
            .into_iter()
            .filter_map(|id| {
                let bounds = self.element_bounds(&format!("session-group:{host}:{id}"))?;
                Some((id, bounds))
            })
            .collect()
    }

    /// Where a dragged session would go: before the first row whose middle is below the
    /// pointer, among its own host's rows.
    fn session_landing(&self, host: usize, id: &str, at: Point<Pixels>) -> Option<Landing> {
        let preview = self
            .session_preview
            .as_ref()
            .filter(|preview| preview.host == host)?;
        let spots = &preview.spots;
        let first = spots.first()?.1;
        let last = spots.last()?.1;
        let slack = px(16.);
        let over = at.x >= first.left() - slack
            && at.x <= first.right() + slack
            && at.y >= first.top() - slack
            && at.y <= last.bottom() + slack;
        if !over {
            return None;
        }
        let before = spots
            .iter()
            .find(|(other, b)| other != id && at.y < b.center().y)
            .map(|(id, _)| id.clone());
        Some(Landing::SessionBefore(before))
    }

    fn update_session_preview(&mut self, host: usize, id: &str, at: Point<Pixels>) {
        if self.session_preview.is_none() {
            let spots = self.session_spots(host);
            self.session_preview = Some(SessionPreview {
                host,
                grabbed: id.to_owned(),
                grab_offset: None,
                order: spots.iter().map(|(id, _)| id.clone()).collect(),
                spots,
                offsets: HashMap::new(),
                started: Instant::now(),
                pending: false,
            });
        }
        let landing = self.session_landing(host, id, at);
        let valid = landing.is_some();
        let start = self.moving.as_ref().map_or(at.y, |moving| moving.start.y);
        let preview = self.session_preview.as_mut().unwrap();
        let mut order: Vec<_> = preview.spots.iter().map(|(key, _)| key.clone()).collect();
        if let Some(Landing::SessionBefore(before)) = landing
            && let Some(from) = order.iter().position(|key| key == id)
        {
            let key = order.remove(from);
            let to = before.and_then(|before| order.iter().position(|key| *key == before));
            order.insert(to.unwrap_or(order.len()), key);
        }
        preview.retarget(order, self.settings.interface.animation);
        preview.grab_offset = if valid {
            let base: Vec<_> = preview.spots.iter().map(|(id, _)| id.clone()).collect();
            preview
                .positions(&base)
                .get(id)
                .map(|origin| *origin + (at.y - start) - preview.positions(&preview.order)[id])
        } else {
            None
        };
    }

    pub(super) fn session_preview_rows(
        &self,
        host: usize,
        sessions: &[view::SessionSummary],
    ) -> Option<SessionPreviewRows<'_>> {
        let preview = self
            .session_preview
            .as_ref()
            .filter(|preview| preview.host == host)?;
        let ids = || sessions.iter().map(|session| &session.session_id);
        if !(ids().eq(preview.spots.iter().map(|(id, _)| id))
            || preview.pending && ids().eq(preview.order.iter()))
        {
            return None;
        }
        let remaining = preview.remaining(self.settings.interface.animation);
        Some(SessionPreviewRows {
            order: &preview.order,
            preview,
            remaining,
            animating: remaining > 0.,
        })
    }

    pub(super) fn settle_session_preview(&mut self) {
        if let Some(preview) = &self.session_preview
            && preview.pending
        {
            let order = self.session_order(preview.host);
            let at_target = order == preview.order;
            let at_source = order.iter().eq(preview.spots.iter().map(|(id, _)| id));
            if (at_target && preview.remaining(self.settings.interface.animation) == 0.)
                || (!at_source && !at_target)
            {
                self.session_preview = None;
            }
        }
    }

    /// The pointer moved with a press held: the drag starts or follows it. Returns whether a
    /// drag is under way.
    pub(super) fn drag_moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(moving) = &mut self.moving else {
            return false;
        };
        moving.at = at;
        if !moving.active {
            let d = at - moving.start;
            let (dx, dy) = (f32::from(d.x), f32::from(d.y));
            if dx * dx + dy * dy < THRESHOLD * THRESHOLD {
                return false;
            }
            moving.active = true;
        }
        if let Grab::Session(host, id) = moving.grab.clone() {
            self.update_session_preview(host, &id, at);
        }
        cx.notify();
        true
    }

    /// Escape: drop the drag without doing anything.
    pub fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let dragging = self.moving.take().is_some_and(|m| {
            if matches!(m.grab, Grab::Session(..))
                && let Some(preview) = &mut self.session_preview
            {
                let original = preview.spots.iter().map(|(id, _)| id.clone()).collect();
                let style = self.settings.interface.animation;
                if preview.order != original {
                    preview.retarget(original, style);
                    preview.grab_offset = None;
                    preview.pending = true;
                } else {
                    preview.settle(style);
                }
            }
            m.active
        });
        if dragging {
            cx.notify();
        }
        dragging
    }

    /// What's being dragged, once a drag is under way.
    pub fn dragging(&self) -> Option<Grab> {
        self.moving
            .as_ref()
            .filter(|m| m.active)
            .map(|m| m.grab.clone())
    }

    /// Where the drag would land if released now.
    pub fn drag_landing(&self) -> Option<Landing> {
        let moving = self.moving.as_ref().filter(|m| m.active)?;
        self.landing(&moving.grab, moving.at)
    }

    /// The tabs of the shown session where they're drawn: the tab bar's, else the sidebar's.
    fn tab_spots(&self) -> Vec<TabSpot> {
        let model = self.model();
        let Some(view) = model.view() else {
            return Vec::new();
        };
        for (prefix, vertical) in [("tab:", false), ("side-tab:", true)] {
            let spots: Vec<TabSpot> = view
                .windows
                .iter()
                .filter_map(|w| {
                    let bounds = self.element_bounds(&format!("{prefix}{}", w.id()))?;
                    Some(TabSpot {
                        id: w.id().to_owned(),
                        bounds,
                        vertical,
                    })
                })
                .collect();
            if spots.len() == view.windows.len() && !spots.is_empty() {
                return spots;
            }
        }
        Vec::new()
    }

    fn landing(&self, grab: &Grab, at: Point<Pixels>) -> Option<Landing> {
        if let Grab::Session(host, id) = grab {
            return self.session_landing(*host, id, at);
        }
        let spots = self.tab_spots();
        // Over the tab strip: its row (or column), with some room either side.
        let over_strip = spots.first().is_some_and(|first| {
            let last = &spots[spots.len() - 1];
            let slack = px(12.);
            if first.vertical {
                at.x >= first.bounds.left() - slack
                    && at.x <= first.bounds.right() + slack
                    && at.y >= first.bounds.top() - slack
                    && at.y <= last.bounds.bottom() + px(40.)
            } else {
                at.y >= first.bounds.top() - slack
                    && at.y <= first.bounds.bottom() + slack
                    && at.x >= first.bounds.left() - slack
                    && at.x <= last.bounds.right() + px(80.)
            }
        });
        // How far along a tab the pointer is, from 0 at its start to 1 at its end.
        let along = |b: &Bounds<Pixels>, vertical: bool| -> f32 {
            if vertical {
                (at.y - b.top()) / b.size.height.max(px(1.))
            } else {
                (at.x - b.left()) / b.size.width.max(px(1.))
            }
        };
        match grab {
            // Handled above, among the session rows.
            Grab::Session(..) => None,
            Grab::Tab(tab) => {
                if !over_strip {
                    return None;
                }
                let before = spots
                    .iter()
                    .find(|s| along(&s.bounds, s.vertical) < 0.5)
                    .map(|s| s.id.clone());
                // Dropping it where it already is changes nothing.
                let at_index = spots.iter().position(|s| s.id == *tab)?;
                let next = spots.get(at_index + 1).map(|s| s.id.clone());
                if before.as_deref() == Some(tab.as_str()) || before == next {
                    return None;
                }
                Some(Landing::TabBefore(before))
            }
            Grab::Pane(block) => {
                if over_strip {
                    let model = self.model();
                    let current = model.view().and_then(|v| v.active_window_id.clone());
                    for spot in &spots {
                        let t = along(&spot.bounds, spot.vertical);
                        if !(0.0..=1.0).contains(&t) {
                            continue;
                        }
                        return if (0.25..=0.75).contains(&t) {
                            (current.as_deref() != Some(spot.id.as_str()))
                                .then(|| Landing::IntoTab(spot.id.clone()))
                        } else if t < 0.25 {
                            Some(Landing::NewTab(Some(spot.id.clone())))
                        } else {
                            let index = spots.iter().position(|s| s.id == spot.id)?;
                            Some(Landing::NewTab(spots.get(index + 1).map(|s| s.id.clone())))
                        };
                    }
                    return Some(Landing::NewTab(None));
                }
                let (anchor, rect) = self
                    .pane_rects()
                    .into_iter()
                    .rev()
                    .find(|(_, b)| b.contains(&at))?;
                if anchor == *block {
                    return None;
                }
                let x = (at.x - rect.left()) / rect.size.width.max(px(1.));
                let y = (at.y - rect.top()) / rect.size.height.max(px(1.));
                let side = [
                    (x, Side::Left),
                    (1. - x, Side::Right),
                    (y, Side::Top),
                    (1. - y, Side::Bottom),
                ]
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, side)| side)?;
                Some(Landing::Beside { anchor, side })
            }
        }
    }

    /// Each pane of the tab on screen and where it's drawn, in window coordinates, floating
    /// panes last.
    fn pane_rects(&self) -> Vec<(String, Bounds<Pixels>)> {
        let model = self.model();
        let Some(window) = model.view().and_then(view::View::active_window) else {
            return Vec::new();
        };
        let area = self.area;
        let mut panes = window.panes();
        panes.sort_by_key(|p| p.floating.is_some());
        panes
            .into_iter()
            .map(|p| {
                let origin = point(
                    area.origin.x + area.size.width * p.rect.x as f32,
                    area.origin.y + area.size.height * p.rect.y as f32,
                );
                let extent = size(
                    area.size.width * p.rect.w as f32,
                    area.size.height * p.rect.h as f32,
                );
                (p.block_id, Bounds::new(origin, extent))
            })
            .collect()
    }

    /// Where a pane of the tab on screen is drawn, in window coordinates.
    pub fn pane_bounds(&self, block: &str) -> Option<Bounds<Pixels>> {
        self.pane_rects()
            .into_iter()
            .find(|(id, _)| id == block)
            .map(|(_, b)| b)
    }

    /// The press ended: a drag lands where it shows; a press that never moved is a click.
    /// Returns whether a drag ended.
    pub(super) fn drag_released(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(moving) = self.moving.take() else {
            return false;
        };
        if !moving.active {
            return false;
        }
        cx.notify();
        if let Grab::Session(host, _) = &moving.grab
            && let Some(preview) = &self.session_preview
            && !self
                .session_order(*host)
                .iter()
                .eq(preview.spots.iter().map(|(id, _)| id))
        {
            self.session_preview = None;
            return true;
        }
        let Some(landing) = self.landing(&moving.grab, moving.at) else {
            if matches!(moving.grab, Grab::Session(..)) {
                self.session_preview = None;
            }
            return true;
        };
        let session = moving.session;
        match (moving.grab, landing) {
            (Grab::Session(host, id), Landing::SessionBefore(before)) => {
                if let Some(preview) = &mut self.session_preview
                    && !preview
                        .order
                        .iter()
                        .eq(preview.spots.iter().map(|(id, _)| id))
                {
                    preview.settle(self.settings.interface.animation);
                    let started = preview.started;
                    self.place_session(host, &id, before);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(2)).await;
                        let _ = this.update(cx, |ws, cx| {
                            if ws
                                .session_preview
                                .as_ref()
                                .is_some_and(|p| p.pending && p.started == started)
                            {
                                ws.session_preview = None;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                } else if let Some(preview) = &mut self.session_preview {
                    preview.settle(self.settings.interface.animation);
                }
            }
            (Grab::Tab(tab), Landing::TabBefore(before)) => {
                let mut payload = json!({"session_id": session, "window_id": tab});
                if let Some(before) = before {
                    payload["before_window_id"] = json!(before);
                }
                self.call("session.move_window", payload);
            }
            (Grab::Pane(block), Landing::IntoTab(tab)) => {
                let model = self.model();
                let anchor = model.view().and_then(|v| v.window(&tab)).and_then(|w| {
                    w.focused_block_id
                        .clone()
                        .or_else(|| w.blocks().find_map(|b| b.block_id.clone()))
                });
                if let Some(anchor) = anchor {
                    self.call(
                        "session.move_block",
                        json!({"session_id": session, "block_id": block, "anchor_block_id": anchor}),
                    );
                }
            }
            (Grab::Pane(block), Landing::NewTab(before)) => {
                self.move_pane_to_new_tab(session, block, before, cx)
            }
            (Grab::Pane(block), Landing::Beside { anchor, side }) => {
                let (direction, place) = match side {
                    Side::Left => ("horizontal", "before"),
                    Side::Right => ("horizontal", "after"),
                    Side::Top => ("vertical", "before"),
                    Side::Bottom => ("vertical", "after"),
                };
                self.call(
                    "session.move_block",
                    json!({"session_id": session, "block_id": block, "anchor_block_id": anchor,
                        "direction": direction, "side": place}),
                );
            }
            _ => {}
        }
        true
    }

    /// Move a pane into a new tab of its own, placed before `before` (or last). A new tab
    /// starts with a shell; the pane moves in beside it and the shell closes.
    pub(super) fn move_pane_to_new_tab(
        &mut self,
        session: String,
        block: String,
        before: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let local = self.host().handle.endpoint.is_local();
        let host = self.host().handle.clone();
        cx.spawn(async move |_, _| {
            let Ok(created) = host
                .request(
                    "session.new_window",
                    json!({"session_id": session, "layout": shell_layout_for(local, None), "focus": true}),
                )
                .await
            else {
                return;
            };
            let Some(placeholder) = created["block_ids"][0].as_str().map(str::to_owned) else {
                return;
            };
            let window = created["window_id"].as_str().map(str::to_owned);
            if host
                .request(
                    "session.move_block",
                    json!({"session_id": session, "block_id": block, "anchor_block_id": placeholder}),
                )
                .await
                .is_err()
            {
                return;
            }
            let _ = host
                .request(
                    "block.close",
                    json!({"session_id": session, "block_id": placeholder}),
                )
                .await;
            let _ = host
                .request(
                    "session.focus_block",
                    json!({"session_id": session, "block_id": block}),
                )
                .await;
            if let (Some(window), Some(before)) = (window, before) {
                let _ = host
                    .request(
                        "session.move_window",
                        json!({"session_id": session, "window_id": window, "before_window_id": before}),
                    )
                    .await;
            }
        })
        .detach();
    }

    /// The dragged thing under the pointer, and where it would land.
    pub(super) fn render_drag(&self, theme: &Theme) -> Option<AnyElement> {
        let moving = self.moving.as_ref().filter(|m| m.active)?;
        if let Grab::Session(host, id) = &moving.grab {
            if self.session_landing(*host, id, moving.at).is_some() {
                return None;
            }
            return self.render_session_drag(*host, id, moving.at, theme);
        }
        let model = self.model();
        let view = model.view()?;
        let title = match &moving.grab {
            Grab::Session(..) => String::new(),
            Grab::Tab(id) => view
                .window(id)
                .map(|w| titles::tab_title(w, &model))
                .unwrap_or_default(),
            Grab::Pane(block) => view
                .window_of(block)
                .and_then(|w| w.block(block))
                .map(|b| titles::pane_title(b, model.blocks.get(block)))
                .unwrap_or_default(),
        };
        let spots = self.tab_spots();
        let mark = |bounds: Bounds<Pixels>| {
            div()
                .absolute()
                .left(bounds.left())
                .top(bounds.top())
                .w(bounds.size.width)
                .h(bounds.size.height)
        };
        let bar = |before: &Option<String>| -> Option<AnyElement> {
            let (bounds, after) = match before {
                Some(id) => (spots.iter().find(|s| s.id == *id)?.bounds, false),
                None => (spots.last()?.bounds, true),
            };
            let vertical = spots.first()?.vertical;
            let line = if vertical {
                let y = if after { bounds.bottom() } else { bounds.top() };
                Bounds::new(
                    point(bounds.left(), y - px(2.)),
                    size(bounds.size.width, px(3.)),
                )
            } else {
                let x = if after { bounds.right() } else { bounds.left() };
                Bounds::new(
                    point(x - px(4.), bounds.top()),
                    size(px(3.), bounds.size.height),
                )
            };
            Some(
                mark(line)
                    .bg(theme.primary)
                    .rounded_full()
                    .into_any_element(),
            )
        };
        let indicator = match self.landing(&moving.grab, moving.at) {
            Some(Landing::TabBefore(before) | Landing::NewTab(before)) => bar(&before),
            Some(Landing::IntoTab(tab)) => spots.iter().find(|s| s.id == tab).map(|s| {
                mark(s.bounds)
                    .rounded(theme.radius_small())
                    .border_2()
                    .border_color(theme.primary)
                    .bg(theme.primary.opacity(0.15))
                    .into_any_element()
            }),
            Some(Landing::Beside { anchor, side }) => self
                .pane_rects()
                .into_iter()
                .find(|(id, _)| *id == anchor)
                .map(|(_, b)| {
                    let half = match side {
                        Side::Left => Bounds::new(b.origin, size(b.size.width / 2., b.size.height)),
                        Side::Right => Bounds::new(
                            point(b.origin.x + b.size.width / 2., b.origin.y),
                            size(b.size.width / 2., b.size.height),
                        ),
                        Side::Top => Bounds::new(b.origin, size(b.size.width, b.size.height / 2.)),
                        Side::Bottom => Bounds::new(
                            point(b.origin.x, b.origin.y + b.size.height / 2.),
                            size(b.size.width, b.size.height / 2.),
                        ),
                    };
                    mark(half)
                        .rounded(theme.radius())
                        .border_2()
                        .border_color(theme.primary)
                        .bg(theme.primary.opacity(0.18))
                        .into_any_element()
                }),
            Some(Landing::SessionBefore(_)) | None => None,
        };
        let chip = div()
            .absolute()
            .left(moving.at.x + px(12.))
            .top(moving.at.y + px(10.))
            .max_w(px(260.))
            .px(px(10.))
            .py(px(5.))
            .rounded(theme.radius_small())
            .bg(theme.surface_high)
            .when(theme.popup_borders, |d| {
                d.border(theme.border_width).border_color(theme.border)
            })
            .when(theme.popup_shadows, |d| d.shadow_md())
            .font_family(theme.ui_font.clone())
            .text_size(px(12.))
            .text_color(theme.text)
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(title);
        Some(
            deferred(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .children(indicator)
                    .child(chip),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// A session dragged outside its host's list: its name follows the pointer.
    fn render_session_drag(
        &self,
        host: usize,
        id: &str,
        at: Point<Pixels>,
        theme: &Theme,
    ) -> Option<AnyElement> {
        let label = self
            .hosts
            .get(host)?
            .handle
            .snapshot()
            .sessions
            .iter()
            .find(|s| s.session_id == id)
            .map(|s| s.label.clone())
            .unwrap_or_default();
        let chip = div()
            .absolute()
            .left(at.x + px(12.))
            .top(at.y + px(10.))
            .max_w(px(260.))
            .px(px(10.))
            .py(px(5.))
            .rounded(theme.radius_small())
            .bg(theme.surface_high)
            .when(theme.popup_borders, |d| {
                d.border(theme.border_width).border_color(theme.border)
            })
            .when(theme.popup_shadows, |d| d.shadow_md())
            .font_family(theme.ui_font.clone())
            .text_size(px(12.))
            .text_color(theme.text)
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(label);
        Some(
            deferred(div().absolute().top_0().left_0().size_full().child(chip))
                .with_priority(3)
                .into_any_element(),
        )
    }
}
