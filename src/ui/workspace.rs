//! The main window: a toolbar with the session's tabs, a sidebar of hosts and sessions,
//! and the active tab's panes laid out exactly as the server arranges them.
use super::{
    components::{dot, heading, icon_button, list_row, small_icon_button, title},
    fonts::{self, TerminalFont},
    icons::icon,
    terminal::TerminalPane,
    theme::Theme,
    titles,
};
use crate::{
    noctalia,
    rex::{
        activity::ProgramState,
        client::Identity,
        endpoint::Endpoint,
        host::{HostCommand, HostHandle, HostModel, shell_layout},
        view::{self, Rect},
    },
    settings::{HostConfig, PaneHeaders, PaneStyle, Settings},
};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, FocusHandle, Focusable, Hsla, MouseButton,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Task, Window, canvas, div,
    prelude::*, px, relative,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct HostEntry {
    pub config: HostConfig,
    pub handle: HostHandle,
}

enum Drag {
    Divider {
        session: String,
        divider: view::Divider,
        sent: Instant,
        at: Point<Pixels>,
    },
    Layer {
        session: String,
        layer_id: String,
        start: Point<Pixels>,
        origin: Rect,
        bounds: Rect,
    },
}

pub struct Workspace {
    pub settings: Settings,
    settings_path: Option<PathBuf>,
    pub hosts: Vec<HostEntry>,
    pub active: usize,
    panes: HashMap<String, Entity<TerminalPane>>,
    font: TerminalFont,
    focus: FocusHandle,
    /// The pane area in window coordinates, from the last paint.
    area: Bounds<Pixels>,
    drag: Option<Drag>,
    /// The block this client last focused for the server's focused block.
    focused: Option<String>,
    /// Sessions whose tabs are listed in the sidebar.
    expanded: HashSet<String>,
    pub notice: Option<String>,
    _tasks: Vec<Task<()>>,
    _watcher: Option<noctalia::Watcher>,
}

fn endpoint_for(config: &HostConfig) -> Endpoint {
    if config.is_local() {
        Endpoint::local()
    } else {
        Endpoint::parse(&config.endpoint).unwrap_or_else(|_| Endpoint::local())
    }
}

pub fn terminal_font(settings: &Settings) -> TerminalFont {
    let mut font = fonts::default_terminal_font();
    if let Some(family) = settings
        .terminal
        .font_family
        .clone()
        .filter(|f| !f.trim().is_empty())
    {
        font.family = family;
    }
    if let Some(size) = settings.terminal.font_size {
        font.size = size.clamp(6., 72.);
    }
    font.line_height = settings.terminal.line_height.clamp(0.8, 2.0);
    font
}

impl Workspace {
    pub fn new(
        settings: Settings,
        settings_path: Option<PathBuf>,
        watch_theme: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let identity = Identity {
            label: settings.client_label.clone(),
            ..Default::default()
        };
        let active = settings
            .last_host
            .as_ref()
            .and_then(|id| settings.hosts.iter().position(|h| &h.id == id))
            .unwrap_or(0);
        let mut tasks = Vec::new();
        let theme = Theme::get(cx).terminal_theme();
        let hosts: Vec<HostEntry> = settings
            .hosts
            .iter()
            .enumerate()
            .map(|(index, config)| {
                let preferred = (index == active)
                    .then(|| settings.last_session.clone())
                    .flatten();
                let handle = HostHandle::spawn(endpoint_for(config), identity.clone(), preferred);
                handle.send(HostCommand::TerminalTheme(Some(theme.clone())));
                handle.send(HostCommand::Visible(index == active));
                let wake = handle.wake.clone();
                tasks.push(cx.spawn_in(window, async move |this, cx| {
                    while wake.recv_async().await.is_ok() {
                        if this
                            .update_in(cx, |ws, window, cx| ws.sync(window, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                }));
                HostEntry {
                    config: config.clone(),
                    handle,
                }
            })
            .collect();

        let watcher = if watch_theme {
            let (themes, received) = flume::unbounded();
            let current = noctalia::load();
            let watcher = noctalia::watch(&current, move |theme| {
                let _ = themes.send(theme);
            })
            .ok();
            tasks.push(cx.spawn(async move |this, cx| {
                while let Ok(theme) = received.recv_async().await {
                    let theme = Theme::from_noctalia(&theme);
                    let applied = this.update(cx, |ws, cx| {
                        let terminal = theme.terminal_theme();
                        for host in &ws.hosts {
                            host.handle
                                .send(HostCommand::TerminalTheme(Some(terminal.clone())));
                        }
                        cx.set_global(theme);
                        cx.refresh_windows();
                    });
                    if applied.is_err() {
                        break;
                    }
                }
            }));
            watcher
        } else {
            None
        };

        Self {
            font: terminal_font(&settings),
            settings,
            settings_path,
            hosts,
            active,
            panes: HashMap::new(),
            focus: cx.focus_handle(),
            area: Bounds::default(),
            drag: None,
            focused: None,
            expanded: HashSet::new(),
            notice: None,
            _tasks: tasks,
            _watcher: watcher,
        }
    }

    pub fn host(&self) -> &HostEntry {
        &self.hosts[self.active]
    }

    pub fn model(&self) -> HostModel {
        self.host().handle.snapshot()
    }

    fn save_settings(&self) {
        if let Some(path) = &self.settings_path
            && let Err(error) = self.settings.save(path)
        {
            eprintln!("rexia: couldn't save settings: {error:#}");
        }
    }

    /// Bring panes in line with the active host's model after it changed.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model();
        if let Some(message) = model.notices.last().cloned() {
            self.notice = Some(message);
            self.host()
                .handle
                .model
                .lock()
                .expect("host model")
                .notices
                .clear();
        }
        let visible: Vec<(String, String)> = model
            .view()
            .and_then(view::View::active_window)
            .map(|w| {
                w.panes()
                    .into_iter()
                    .map(|p| (model.selected.clone().unwrap_or_default(), p.block_id))
                    .collect()
            })
            .unwrap_or_default();
        self.panes
            .retain(|block, _| visible.iter().any(|(_, id)| id == block));
        let copy_on_select = self.settings.terminal.copy_on_select;
        let padding = Point::new(
            px(self.settings.terminal.padding_x),
            px(self.settings.terminal.padding_y),
        );
        for (session, block) in visible {
            let pane = self.panes.entry(block.clone()).or_insert_with(|| {
                let host = self.hosts[self.active].handle.clone();
                let font = self.font.clone();
                cx.new(|cx| {
                    let mut pane = TerminalPane::new(host, session, block, font, cx);
                    pane.copy_on_select = copy_on_select;
                    pane.padding = padding;
                    pane
                })
            });
            pane.update(cx, |pane, cx| {
                if pane.changed() {
                    cx.notify();
                }
            });
        }
        let focused = model
            .view()
            .and_then(view::View::active_window)
            .and_then(|w| w.focused_block_id.clone());
        if focused != self.focused
            && let Some(block) = &focused
            && let Some(pane) = self.panes.get(block)
        {
            let handle = pane.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
            self.focused = focused;
        }
        if let Some(id) = model.selected.clone()
            && self.settings.last_session.as_ref() != Some(&id)
        {
            self.settings.last_session = Some(id);
            self.settings.last_host = Some(self.host().config.id.clone());
            self.save_settings();
        }
        cx.notify();
    }

    fn call(&self, method: &str, payload: Value) {
        self.host().handle.call(method, payload);
    }

    fn session_id(&self) -> Option<String> {
        self.model().selected
    }

    pub fn select_session(&mut self, host: usize, session: String, cx: &mut Context<Self>) {
        if host != self.active {
            self.hosts[self.active]
                .handle
                .send(HostCommand::Visible(false));
            self.active = host;
            self.panes.clear();
            self.focused = None;
            self.hosts[host].handle.send(HostCommand::Visible(true));
        }
        self.hosts[host]
            .handle
            .send(HostCommand::Select(session.clone()));
        self.expanded.insert(session);
        cx.notify();
    }

    pub fn new_session(&mut self, host: usize, cx: &mut Context<Self>) {
        if host != self.active {
            self.hosts[self.active]
                .handle
                .send(HostCommand::Visible(false));
            self.active = host;
            self.panes.clear();
            self.hosts[host].handle.send(HostCommand::Visible(true));
        }
        self.hosts[host].handle.send(HostCommand::CreateSession {
            label: None,
            reply: None,
        });
        cx.notify();
    }

    pub fn new_tab(&mut self, _: &mut Context<Self>) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.new_window",
            json!({"session_id": session, "layout": shell_layout(), "focus": true}),
        );
    }

    pub fn select_tab(&mut self, window_id: String, _: &mut Context<Self>) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.focus_window",
            json!({"session_id": session, "window_id": window_id}),
        );
    }

    pub fn close_tab(&mut self, window_id: String, _: &mut Context<Self>) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.close_window",
            json!({"session_id": session, "window_id": window_id}),
        );
    }

    /// Split the focused pane (or `anchor`): `horizontal` puts the new pane beside it.
    pub fn split(&mut self, anchor: Option<String>, horizontal: bool, after: bool) {
        let Some(session) = self.session_id() else {
            return;
        };
        let mut payload = json!({
            "session_id": session,
            "direction": if horizontal { "horizontal" } else { "vertical" },
            "side": if after { "after" } else { "before" },
            "layout": shell_layout(),
            "focus": true,
        });
        if let Some(anchor) = anchor {
            payload["anchor_id"] = json!(anchor);
        }
        self.call("session.new_split", payload);
    }

    pub fn close_pane(&mut self, block: String) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "block.close",
            json!({"session_id": session, "block_id": block}),
        );
    }

    pub fn toggle_zoom(&mut self, block: String) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.zoom_block",
            json!({"session_id": session, "block_id": block}),
        );
    }

    /// Float a tiled pane over its window, or dock a floating one beside the focused tile.
    pub fn toggle_float(&mut self, block: String) {
        let model = self.model();
        let Some(session) = model.selected.clone() else {
            return;
        };
        let Some(window) = model.view().and_then(view::View::active_window) else {
            return;
        };
        let floating = window.layer_of(&block).is_some_and(|l| l.kind != "tiled");
        if floating {
            let anchor = window
                .layers
                .iter()
                .filter(|l| l.kind == "tiled")
                .find_map(|l| l.layout.as_ref().and_then(view::first_block))
                .map(str::to_owned);
            match anchor {
                Some(anchor) => self.call(
                    "session.move_block",
                    json!({"session_id": session, "block_id": block, "anchor_block_id": anchor}),
                ),
                None => self.notice = Some("There's no tiled pane to dock beside".into()),
            }
        } else {
            self.call(
                "session.float_block",
                json!({"session_id": session, "block_id": block, "bounds": [0.15, 0.12, 0.7, 0.72]}),
            );
        }
    }

    pub fn restore_layer(&mut self, layer: String) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.set_layer_state",
            json!({"session_id": session, "layer_id": layer, "state": "normal", "focus": true}),
        );
    }

    pub fn minimize_layer(&mut self, layer: String) {
        let Some(session) = self.session_id() else {
            return;
        };
        self.call(
            "session.set_layer_state",
            json!({"session_id": session, "layer_id": layer, "state": "minimized"}),
        );
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.settings.interface.sidebar = !self.settings.interface.sidebar;
        self.save_settings();
        cx.notify();
    }

    fn fraction(&self, at: Point<Pixels>) -> (f64, f64) {
        let size = self.area.size;
        let x = f64::from(f32::from(
            (at.x - self.area.origin.x) / size.width.max(px(1.)),
        ));
        let y = f64::from(f32::from(
            (at.y - self.area.origin.y) / size.height.max(px(1.)),
        ));
        (x, y)
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (fx, fy) = self.fraction(event.position);
        let start_fraction = match &self.drag {
            Some(Drag::Layer { start, .. }) => Some(self.fraction(*start)),
            _ => None,
        };
        let mut send = None;
        match &mut self.drag {
            Some(Drag::Divider {
                session,
                divider,
                sent,
                at,
            }) => {
                *at = event.position;
                let rect = divider.rect;
                let ratio = if divider.horizontal {
                    (fx - rect.x) / rect.w.max(1e-6)
                } else {
                    (fy - rect.y) / rect.h.max(1e-6)
                };
                divider.ratio = ratio.clamp(0.05, 0.95);
                // Resize as the pointer moves, at most 20 times a second.
                if sent.elapsed() > Duration::from_millis(50) {
                    *sent = Instant::now();
                    send = Some(resize_pane(session, divider));
                }
            }
            Some(Drag::Layer { origin, bounds, .. }) => {
                let (sx, sy) = start_fraction.unwrap_or((fx, fy));
                bounds.x = (origin.x + fx - sx).clamp(0., 1. - origin.w);
                bounds.y = (origin.y + fy - sy).clamp(0., 1. - origin.h);
            }
            None => return,
        }
        if let Some((method, payload)) = send {
            self.call(method, payload);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Divider {
                session, divider, ..
            }) => {
                let (method, payload) = resize_pane(&session, &divider);
                self.call(method, payload);
            }
            Some(Drag::Layer {
                session,
                layer_id,
                bounds,
                ..
            }) => self.call(
                "session.resize_layer",
                json!({"session_id": session, "layer_id": layer_id,
                    "bounds": [bounds.x, bounds.y, bounds.w, bounds.h]}),
            ),
            None => return,
        }
        cx.notify();
    }

    fn render_toolbar(
        &self,
        model: &HostModel,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = model.view();
        let session_label = view
            .map(|v| v.label.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "No session".into());
        let active_window = view.and_then(|v| v.active_window_id.clone());
        let tabs: Vec<AnyElement> = view
            .map(|v| v.windows.as_slice())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(index, window)| {
                let id = window.id().to_owned();
                let active = active_window.as_deref() == Some(window.id());
                let label = titles::tab_title(window, model);
                let activity = titles::tab_activity(window, model).map(|r| (r.state, r.badge()));
                let focused_state = window
                    .focused_block_id
                    .as_ref()
                    .and_then(|b| model.blocks.get(b));
                let (fg, bg) = if active {
                    (theme.on_primary, theme.primary)
                } else {
                    (theme.text, theme.background)
                };
                let select_id = id.clone();
                let close_id = id.clone();
                div()
                    .id(SharedString::from(format!("tab-{index}")))
                    .group("tab")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .min_w(px(120.))
                    .max_w(px(240.))
                    .px(px(10.))
                    .rounded(theme.radius_small())
                    .bg(bg)
                    .text_color(fg)
                    .when(!active && theme.button_borders, |d| {
                        d.border(theme.border_width).border_color(theme.border)
                    })
                    .when(!active, |d| d.hover(|d| d.bg(theme.overlay(0.06))))
                    .cursor_pointer()
                    .on_click(cx.listener(move |ws, _, _, cx| ws.select_tab(select_id.clone(), cx)))
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(move |ws, _, _, cx| ws.close_tab(close_id.clone(), cx)),
                    )
                    .child(
                        icon(titles::program_icon(focused_state))
                            .size(px(15.))
                            .text_color(fg),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(13.))
                            .child(label),
                    )
                    .children(
                        activity.map(|(state, badge)| activity_mark(state, badge, active, theme)),
                    )
                    .child(div().invisible().group_hover("tab", |d| d.visible()).child(
                        small_icon_button(
                            SharedString::from(format!("tab-close-{index}")),
                            "x",
                            fg,
                            theme,
                            cx.listener(move |ws, _, _, cx| ws.close_tab(id.clone(), cx)),
                        ),
                    ))
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(48.))
            .px(px(10.))
            .border_b(theme.border_width)
            .border_color(theme.border)
            .child(icon_button(
                "toggle-sidebar",
                "layout-sidebar",
                theme,
                cx.listener(|ws, _, _, cx| ws.toggle_sidebar(cx)),
            ))
            .child(
                div()
                    .flex_none()
                    .mx(px(4.))
                    .child(title(session_label, theme)),
            )
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .overflow_x_scroll()
                    .children(tabs)
                    .child(icon_button(
                        "new-tab",
                        "plus",
                        theme,
                        cx.listener(|ws, _, _, cx| ws.new_tab(cx)),
                    )),
            )
            .child(icon_button(
                "split-right",
                "layout-columns",
                theme,
                cx.listener(|ws, _, _, _| ws.split(None, true, true)),
            ))
            .child(icon_button(
                "split-down",
                "layout-rows",
                theme,
                cx.listener(|ws, _, _, _| ws.split(None, false, true)),
            ))
            .into_any_element()
    }

    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().flex().flex_col().gap(px(2.));
        for (index, host) in self.hosts.iter().enumerate() {
            let model = host.handle.snapshot();
            let status = match &model.connection {
                crate::rex::host::Connection::Connected => None,
                crate::rex::host::Connection::Connecting => Some("Connecting…".to_owned()),
                crate::rex::host::Connection::Disconnected(reason) => Some(reason.clone()),
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(heading(host.config.label.clone(), theme))
                    .child(small_icon_button(
                        SharedString::from(format!("host-new-{index}")),
                        "plus",
                        theme.text_muted,
                        theme,
                        cx.listener(move |ws, _, _, cx| ws.new_session(index, cx)),
                    )),
            );
            if let Some(status) = status {
                list = list.child(
                    div()
                        .px(px(10.))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .text_color(theme.error)
                        .child(status),
                );
            }
            for session in &model.sessions {
                let selected =
                    index == self.active && model.selected.as_ref() == Some(&session.session_id);
                let id = session.session_id.clone();
                let activity = titles::session_activity(&session.session_id, &model)
                    .map(|r| (r.state, r.badge()));
                let windows = model
                    .views
                    .get(&session.session_id)
                    .map(|v| v.windows.len())
                    .unwrap_or(0);
                list = list.child(
                    list_row(
                        SharedString::from(format!("session-{index}-{}", session.session_id)),
                        selected,
                        theme,
                    )
                    .child(icon("layout-columns").size(px(16.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(14.))
                            .child(session.label.clone()),
                    )
                    .children(
                        activity.map(|(state, badge)| activity_mark(state, badge, selected, theme)),
                    )
                    .when(windows > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(if selected {
                                    theme.on_primary
                                } else {
                                    theme.text_muted
                                })
                                .child(windows.to_string()),
                        )
                    })
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.select_session(index, id.clone(), cx)),
                    ),
                );
                if selected && let Some(view) = model.views.get(&session.session_id) {
                    for window in &view.windows {
                        let active = view.active_window_id.as_deref() == Some(window.id());
                        let window_id = window.id().to_owned();
                        let state = window
                            .focused_block_id
                            .as_ref()
                            .and_then(|b| model.blocks.get(b));
                        list = list.child(
                            div()
                                .id(SharedString::from(format!("side-tab-{window_id}")))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .ml(px(18.))
                                .px(px(10.))
                                .h(px(28.))
                                .rounded(theme.radius_small())
                                .cursor_pointer()
                                .text_size(px(13.))
                                .when(active, |d| d.bg(theme.overlay(0.1)).text_color(theme.text))
                                .when(!active, |d| {
                                    d.text_color(theme.text_muted)
                                        .hover(|d| d.bg(theme.overlay(0.06)))
                                })
                                .child(icon(titles::program_icon(state)).size(px(14.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(titles::tab_title(window, &model)),
                                )
                                .on_click(cx.listener(move |ws, _, _, cx| {
                                    ws.select_tab(window_id.clone(), cx)
                                })),
                        );
                    }
                }
            }
        }
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(260.))
            .h_full()
            .p(px(10.))
            .gap(px(6.))
            .border_r(theme.border_width)
            .border_color(theme.border)
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(6.))
                    .pb(px(4.))
                    .child(icon("server").size(px(20.)).text_color(theme.primary))
                    .child(title("Sessions", theme)),
            )
            .child(list)
            .into_any_element()
    }

    fn render_panes(
        &mut self,
        model: &HostModel,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let tracker = canvas(
            move |bounds, _, cx| entity.update(cx, |ws, _| ws.area = bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let Some(window) = model.view().and_then(view::View::active_window).cloned() else {
            let message = match &model.connection {
                crate::rex::host::Connection::Connected if model.sessions.is_empty() => {
                    "No sessions on this host"
                }
                crate::rex::host::Connection::Connected => "Choose a session",
                _ => "Connecting…",
            };
            return div()
                .flex_1()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_muted)
                .child(tracker)
                .child(message)
                .into_any_element();
        };
        let session = model.selected.clone().unwrap_or_default();
        let gap = match self.settings.interface.pane_style {
            PaneStyle::Comfortable => 8.,
            PaneStyle::Compact => 2.,
        };
        let headers = self.settings.interface.pane_headers;
        let focused = window.focused_block_id.clone();
        let zoomed = window.zoomed_block_id.is_some();
        let dragged_layer = match &self.drag {
            Some(Drag::Layer {
                layer_id,
                origin,
                bounds,
                ..
            }) => Some((layer_id.clone(), *origin, *bounds)),
            _ => None,
        };

        let mut area = div().flex_1().relative().overflow_hidden().child(tracker);
        for pane in window.panes() {
            let Some(terminal) = self.panes.get(&pane.block_id).cloned() else {
                continue;
            };
            let mut rect = pane.rect;
            if let (Some(layer), Some((dragged, origin, bounds))) = (&pane.floating, &dragged_layer)
                && layer == dragged
            {
                rect.x += bounds.x - origin.x;
                rect.y += bounds.y - origin.y;
            }
            let block = window.block(&pane.block_id).cloned().unwrap_or_default();
            let state = model.blocks.get(&pane.block_id);
            let is_focused = focused.as_deref() == Some(pane.block_id.as_str());
            let floating = pane.floating.clone();
            let header = (headers != PaneHeaders::Hidden).then(|| {
                self.pane_header(
                    &pane.block_id,
                    &block,
                    state,
                    floating.as_deref(),
                    zoomed,
                    is_focused,
                    &session,
                    rect,
                    theme,
                    cx,
                )
            });
            let border = if is_focused
                && self.settings.interface.highlight_active_pane
                && window.panes().len() > 1
            {
                theme.primary
            } else {
                theme.border
            };
            let card = div()
                .size_full()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(theme.radius())
                .when(theme.card_borders || floating.is_some(), |d| {
                    d.border(theme.border_width).border_color(border)
                })
                .when(floating.is_some() && theme.popup_shadows, |d| d.shadow_lg())
                .bg(theme.background)
                .group(SharedString::from(format!("pane-{}", pane.block_id)))
                .children(header)
                .child(div().flex_1().min_h_0().child(terminal));
            area = area.child(
                div()
                    .absolute()
                    .left(relative(rect.x as f32))
                    .top(relative(rect.y as f32))
                    .w(relative(rect.w as f32))
                    .h(relative(rect.h as f32))
                    .p(px(gap / 2.))
                    .child(card),
            );
        }
        for divider in window.dividers() {
            area = area.child(self.divider(&session, divider, theme, cx));
        }
        if let Some(Drag::Divider { divider, .. }) = &self.drag {
            area = area.child(divider_guide(divider, theme));
        }
        let minimized = window.minimized_layers();
        if !minimized.is_empty() {
            let chips = minimized.into_iter().filter_map(|layer| {
                let layer_id = layer.layer_id.clone()?;
                let block = layer.blocks.first()?;
                let label = titles::pane_title(
                    block,
                    block.block_id.as_ref().and_then(|b| model.blocks.get(b)),
                );
                Some(
                    div()
                        .id(SharedString::from(format!("restore-{layer_id}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(theme.radius_small())
                        .bg(theme.surface_high)
                        .border(theme.border_width)
                        .border_color(theme.border)
                        .cursor_pointer()
                        .text_size(px(12.))
                        .child(icon("picture-in-picture").size(px(14.)))
                        .child(label)
                        .on_click(
                            cx.listener(move |ws, _, _, _| ws.restore_layer(layer_id.clone())),
                        ),
                )
            });
            area = area.child(
                div()
                    .absolute()
                    .bottom(px(12.))
                    .right(px(12.))
                    .flex()
                    .gap(px(6.))
                    .children(chips),
            );
        }
        area.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn pane_header(
        &self,
        block_id: &str,
        block: &view::Block,
        state: Option<&crate::rex::host::BlockState>,
        floating: Option<&str>,
        zoomed: bool,
        focused: bool,
        session: &str,
        rect: Rect,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = titles::pane_title(block, state);
        let status = state
            .and_then(|s| s.status())
            .filter(|r| r.state != ProgramState::Idle);
        let muted = if focused {
            theme.text
        } else {
            theme.text_muted
        };
        let id = block_id.to_owned();
        let hover_only = self.settings.interface.pane_headers == PaneHeaders::Hover;
        let group = SharedString::from(format!("pane-{block_id}"));
        let mut header = div()
            .id(SharedString::from(format!("header-{block_id}")))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(8.))
            .bg(theme.surface)
            .border_b(theme.border_width)
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(muted)
            .when(hover_only, |d| {
                d.invisible().group_hover(group.clone(), |d| d.visible())
            })
            .child(icon(titles::program_icon(state)).size(px(14.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label),
            )
            .children(status.map(|r| activity_mark(r.state, r.badge(), false, theme)));
        if let Some(layer) = floating {
            let layer_id = layer.to_owned();
            let session = session.to_owned();
            header = header.cursor_move().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, event: &gpui::MouseDownEvent, _, cx| {
                    ws.drag = Some(Drag::Layer {
                        session: session.clone(),
                        layer_id: layer_id.clone(),
                        start: event.position,
                        origin: rect,
                        bounds: rect,
                    });
                    cx.stop_propagation();
                }),
            );
            let minimize = layer.to_owned();
            header = header.child(small_icon_button(
                SharedString::from(format!("minimize-{block_id}")),
                "minus",
                muted,
                theme,
                cx.listener(move |ws, _, _, _| ws.minimize_layer(minimize.clone())),
            ));
        }
        let (zoom_id, float_id, close_id, split_id) =
            (id.clone(), id.clone(), id.clone(), id.clone());
        header
            .child(small_icon_button(
                SharedString::from(format!("split-{block_id}")),
                "layout-columns",
                muted,
                theme,
                cx.listener(move |ws, _, _, _| ws.split(Some(split_id.clone()), true, true)),
            ))
            .child(small_icon_button(
                SharedString::from(format!("zoom-{block_id}")),
                if zoomed {
                    "arrows-minimize"
                } else {
                    "arrows-maximize"
                },
                muted,
                theme,
                cx.listener(move |ws, _, _, _| ws.toggle_zoom(zoom_id.clone())),
            ))
            .child(small_icon_button(
                SharedString::from(format!("float-{block_id}")),
                if floating.is_some() {
                    "picture-in-picture-off"
                } else {
                    "picture-in-picture"
                },
                muted,
                theme,
                cx.listener(move |ws, _, _, _| ws.toggle_float(float_id.clone())),
            ))
            .child(small_icon_button(
                SharedString::from(format!("close-{block_id}")),
                "x",
                muted,
                theme,
                cx.listener(move |ws, _, _, _| ws.close_pane(close_id.clone())),
            ))
            .into_any_element()
    }

    fn divider(
        &self,
        session: &str,
        divider: view::Divider,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rect = divider.rect;
        let horizontal = divider.horizontal;
        let id = SharedString::from(format!(
            "divider-{}-{}",
            divider.before_block, divider.after_block
        ));
        let session = session.to_owned();
        let ratio = divider.ratio;
        let handle = div()
            .id(id)
            .absolute()
            .hover(|d| d.bg(theme.overlay(0.12)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, event: &gpui::MouseDownEvent, _, cx| {
                    ws.drag = Some(Drag::Divider {
                        session: session.clone(),
                        divider: divider.clone(),
                        sent: Instant::now(),
                        at: event.position,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        let handle = if horizontal {
            let x = rect.x + rect.w * ratio;
            handle
                .left(relative(x as f32))
                .ml(px(-4.))
                .w(px(8.))
                .top(relative(rect.y as f32))
                .h(relative(rect.h as f32))
                .cursor_col_resize()
        } else {
            let y = rect.y + rect.h * ratio;
            handle
                .top(relative(y as f32))
                .mt(px(-4.))
                .h(px(8.))
                .left(relative(rect.x as f32))
                .w(relative(rect.w as f32))
                .cursor_row_resize()
        };
        handle.into_any_element()
    }
}

fn resize_pane(session: &str, divider: &view::Divider) -> (&'static str, Value) {
    (
        "session.resize_pane",
        json!({
            "session_id": session,
            "direction": if divider.horizontal { "horizontal" } else { "vertical" },
            "before_block_id": divider.before_block,
            "after_block_id": divider.after_block,
            "ratio": divider.ratio,
        }),
    )
}

/// A line where a dragged divider will land.
fn divider_guide(divider: &view::Divider, theme: &Theme) -> AnyElement {
    let rect = divider.rect;
    let line = div().absolute().bg(theme.primary);
    if divider.horizontal {
        let x = rect.x + rect.w * divider.ratio;
        line.left(relative(x as f32))
            .ml(px(-1.))
            .w(px(2.))
            .top(relative(rect.y as f32))
            .h(relative(rect.h as f32))
            .into_any_element()
    } else {
        let y = rect.y + rect.h * divider.ratio;
        line.top(relative(y as f32))
            .mt(px(-1.))
            .h(px(2.))
            .left(relative(rect.x as f32))
            .w(relative(rect.w as f32))
            .into_any_element()
    }
}

fn state_color(state: ProgramState, theme: &Theme) -> Hsla {
    match state {
        ProgramState::Blocked => theme.secondary,
        ProgramState::Error => theme.error,
        ProgramState::Done => theme.tertiary,
        ProgramState::Working => theme.primary,
        ProgramState::Idle => theme.text_muted,
    }
}

/// A dot for working programs; a dot and its word when a program needs the user.
fn activity_mark(
    state: ProgramState,
    badge: &'static str,
    on_primary: bool,
    theme: &Theme,
) -> AnyElement {
    let tint = if on_primary {
        theme.on_primary
    } else {
        state_color(state, theme)
    };
    let mut mark = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .child(dot(tint, 7.));
    if matches!(
        state,
        ProgramState::Blocked | ProgramState::Error | ProgramState::Done
    ) {
        mark = mark.child(div().text_size(px(11.)).text_color(tint).child(badge));
    }
    mark.into_any_element()
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let model = self.model();
        let toolbar = self.render_toolbar(&model, &theme, cx);
        let sidebar = self
            .settings
            .interface
            .sidebar
            .then(|| self.render_sidebar(&theme, cx));
        let panes = self.render_panes(&model, &theme, cx);
        let notice = self.notice.clone().map(|message| {
            div()
                .id("notice")
                .absolute()
                .bottom(px(16.))
                .left(relative(0.5))
                .ml(px(-220.))
                .w(px(440.))
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(10.))
                .rounded(theme.radius())
                .bg(theme.surface_high)
                .when(theme.popup_borders, |d| {
                    d.border(theme.border_width).border_color(theme.border)
                })
                .when(theme.popup_shadows, |d| d.shadow_lg())
                .child(icon("alert-triangle").size(px(16.)).text_color(theme.error))
                .child(div().flex_1().text_size(px(13.)).child(message))
                .child(small_icon_button(
                    "dismiss-notice",
                    "x",
                    theme.text_muted,
                    &theme,
                    cx.listener(|ws, _, _, cx| {
                        ws.notice = None;
                        cx.notify();
                    }),
                ))
        });
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.ui_font.clone())
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .children(sidebar)
                    .child(panes),
            )
            .children(notice)
    }
}
