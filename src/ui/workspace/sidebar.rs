use super::Workspace;
use crate::{
    rex::host::{Connection, HostCommand},
    ui::{
        components::{dot, title},
        icons::icon,
        theme::Theme,
    },
};
use gpui::{
    AnyElement, Context, Div, ElementId, Role, SharedString, Stateful, Toggled, div,
    linear_color_stop, linear_gradient, prelude::*, px,
};

impl Workspace {
    fn sidebar_settings_changed(&self, cx: &mut Context<Self>) {
        self.save_settings();
        cx.notify();
    }

    pub(super) fn show_sidebar_host(&self, id: &str, connection: &Connection) -> bool {
        let sidebar = &self.settings.sidebar;
        sidebar.host_selected(id) && (!sidebar.online_only || *connection == Connection::Connected)
    }

    fn toggle_sidebar_host(&mut self, id: &str, cx: &mut Context<Self>) {
        let hosts = self.settings.sidebar.selected_hosts.get_or_insert_with(|| {
            self.hosts
                .iter()
                .map(|host| host.config.id.clone())
                .collect()
        });
        if !hosts.remove(id) {
            hosts.insert(id.to_owned());
        }
        self.sidebar_settings_changed(cx);
    }

    pub(super) fn toggle_host_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        let collapsed = &mut self.settings.sidebar.collapsed_hosts;
        if !collapsed.remove(id) {
            collapsed.insert(id.to_owned());
        }
        self.sidebar_settings_changed(cx);
    }

    pub(super) fn set_session_expanded(
        &mut self,
        host: usize,
        session: String,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let entry = &self.hosts[host];
        let sidebar = &mut self.settings.sidebar;
        let host = &entry.config.id;
        let changed = sidebar.session_collapsed(host, &session) == expanded;
        if changed {
            sidebar.set_session_collapsed(host, &session, !expanded);
        }
        if expanded {
            self.expanded_sessions
                .insert((host.clone(), session.clone()));
            if !entry.handle.snapshot().views.contains_key(&session) {
                entry.handle.send(HostCommand::LoadSession(session));
            }
        }
        if changed {
            self.sidebar_settings_changed(cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn collapse_other_sessions(
        &mut self,
        session: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for (index, host) in self.hosts.iter().enumerate() {
            for item in &host.handle.snapshot().sessions {
                if index == self.active && Some(item.session_id.as_str()) == session {
                    continue;
                }
                let sidebar = &mut self.settings.sidebar;
                if !sidebar.session_collapsed(&host.config.id, &item.session_id) {
                    sidebar.set_session_collapsed(&host.config.id, &item.session_id, true);
                    changed = true;
                }
            }
        }
        if changed {
            self.sidebar_settings_changed(cx);
        }
    }

    pub(super) fn render_sessions_header(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let sidebar = &self.settings.sidebar;
        let label = if sidebar.online_only {
            "Online Hosts".to_owned()
        } else if let Some(selected) = &sidebar.selected_hosts {
            let count = self
                .hosts
                .iter()
                .filter(|host| selected.contains(&host.config.id))
                .count();
            if count == 1 {
                "1 host".into()
            } else {
                format!("{count} hosts")
            }
        } else {
            "Hosts".to_owned()
        };
        let trigger = filter_button("host-filter", theme)
            .role(Role::Button)
            .aria_label(format!("Filter hosts: {label}"))
            .aria_expanded(self.host_filter_open)
            .gap(px(6.))
            .child(label)
            .child(
                icon(if self.host_filter_open {
                    "chevron-up"
                } else {
                    "chevron-down"
                })
                .size(px(12.))
                .text_color(theme.text),
            )
            .child(self.track_bounds("host-filter".into(), cx))
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.host_filter_open = !ws.host_filter_open;
                cx.notify();
            }));
        let heading = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(6.))
            .child(title(
                if cfg!(debug_assertions) {
                    "Sessions (dev)"
                } else {
                    "Sessions"
                },
                theme,
            ))
            .child(trigger);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(heading)
            .when(self.host_filter_open, |d| {
                d.child(self.render_host_filter(theme, cx))
            })
    }

    fn render_host_filter(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let sidebar = &self.settings.sidebar;
        let online = filter_button("online-hosts", theme)
            .role(Role::CheckBox)
            .aria_label("Online Hosts")
            .aria_toggled(toggled(sidebar.online_only))
            .gap(px(8.))
            .child(checkbox(sidebar.online_only, theme))
            .child("Online Hosts")
            .child(self.track_bounds("host-filter:online".into(), cx))
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.settings.sidebar.online_only = !ws.settings.sidebar.online_only;
                ws.sidebar_settings_changed(cx);
            }));
        let all = filter_button("all-hosts", theme)
            .role(Role::Button)
            .aria_label("Select all hosts")
            .child("Select All Hosts")
            .child(self.track_bounds("host-filter:all".into(), cx))
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.settings.sidebar.selected_hosts = None;
                ws.sidebar_settings_changed(cx);
            }));
        let mut choices = div()
            .flex()
            .flex_col()
            .p(px(4.))
            .rounded(theme.radius())
            .bg(theme.surface)
            .child(online)
            .child(all);
        for host in &self.hosts {
            let id = &host.config.id;
            let label = &host.config.label;
            let selected = sidebar.host_selected(id);
            let connected = host.handle.snapshot().connection == Connection::Connected;
            let checked = filter_button(SharedString::from(format!("check-host-{id}")), theme)
                .role(Role::CheckBox)
                .aria_label(label.clone())
                .aria_toggled(toggled(selected))
                .justify_center()
                .w(px(26.))
                .px_0()
                .child(self.track_bounds(format!("host-filter:check:{id}"), cx))
                .child(checkbox(selected, theme))
                .on_click(cx.listener({
                    let id = id.clone();
                    move |ws, _, _, cx| ws.toggle_sidebar_host(&id, cx)
                }));
            let solo = filter_button(SharedString::from(format!("solo-host-{id}")), theme)
                .role(Role::Button)
                .aria_label(format!("Show only {label}"))
                .aria_description(if connected { "Online" } else { "Offline" })
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .child(self.track_bounds(format!("host-filter:solo:{id}"), cx))
                .child(dot(
                    if connected {
                        theme.primary
                    } else {
                        theme.text_muted
                    },
                    6.,
                ))
                .child(
                    div()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label.clone()),
                )
                .on_click(cx.listener({
                    let id = id.clone();
                    move |ws, _, _, cx| {
                        ws.settings.sidebar.selected_hosts = Some([id.clone()].into());
                        ws.sidebar_settings_changed(cx);
                    }
                }));
            choices = choices.child(div().flex().items_center().child(checked).child(solo));
        }
        choices
    }

    pub(super) fn sidebar_scroll(
        &self,
        id: &'static str,
        content: Div,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        // The spacers travel with the rows; the fades stay over the viewport edges.
        let fade_height = px(14.);
        let fade = |top| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .h(fade_height)
                .when(top, |d| d.top_0())
                .when(!top, |d| d.bottom_0())
                .bg(linear_gradient(
                    if top { 180. } else { 0. },
                    linear_color_stop(theme.background, 0.),
                    linear_color_stop(theme.background.opacity(0.), 1.),
                ))
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                content
                    .id(id)
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px(px(10.))
                    .py(fade_height)
                    .overflow_y_scroll(),
            )
            .child(fade(true).child(self.track_bounds("sidebar-fade:top".into(), cx)))
            .child(fade(false).child(self.track_bounds("sidebar-fade:bottom".into(), cx)))
            .into_any_element()
    }
}

fn filter_button(id: impl Into<ElementId>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .focusable()
        .tab_index(0)
        .flex()
        .items_center()
        .h(px(26.))
        .px(px(6.))
        .rounded(theme.radius_small())
        .border_2()
        .border_color(theme.text.opacity(0.))
        .text_size(px(12.))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(|d| d.bg(theme.overlay(0.08)))
        .focus_visible(|d| d.border_color(theme.primary))
}

fn toggled(checked: bool) -> Toggled {
    if checked {
        Toggled::True
    } else {
        Toggled::False
    }
}

fn checkbox(checked: bool, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(14.))
        .rounded(px(3.))
        .border_1()
        .border_color(theme.text_muted)
        .when(checked, |d| {
            d.bg(theme.primary)
                .border_color(theme.primary)
                .text_color(theme.on_primary)
                .child("✓")
        })
}
