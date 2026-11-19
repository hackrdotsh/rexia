//! The sidebar's tab filter: type to narrow the sessions and tabs of every host to those
//! whose names match, then Enter (or a click) shows the chosen one.
use super::Workspace;
use crate::ui::{
    components::{heading, list_row},
    fuzzy,
    icons::icon,
    input::{InputEvent, TextInput},
    theme::Theme,
    titles,
};
use gpui::{AnyElement, Context, Entity, SharedString, Subscription, Window, div, prelude::*, px};

pub struct TabFilter {
    pub input: Entity<TextInput>,
    pub query: String,
    /// The highlighted match, which Enter shows.
    pub selected: usize,
    _subscription: Subscription,
}

/// One row of filter results: a session, or one of its tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterMatch {
    pub host: usize,
    pub session: String,
    pub window: Option<String>,
    pub title: String,
}

impl TabFilter {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let input = cx.new(|cx| TextInput::new("Filter tabs", cx));
        let subscription = cx.subscribe_in(&input, window, |ws, _, event, window, cx| {
            ws.on_filter_input(event, window, cx)
        });
        Self {
            input,
            query: String::new(),
            selected: 0,
            _subscription: subscription,
        }
    }
}

impl Workspace {
    /// Sessions and tabs whose names match the filter, host by host. A session whose own
    /// name matches brings all its tabs; otherwise only its matching tabs show.
    pub fn filter_matches(&self) -> Vec<FilterMatch> {
        let query = self.filter.query.trim();
        let mut matches = Vec::new();
        if query.is_empty() {
            return matches;
        }
        for (host, entry) in self.hosts.iter().enumerate() {
            let model = entry.handle.snapshot();
            if !self.show_sidebar_host(&entry.config.id, &model.connection) {
                continue;
            }
            for session in &model.sessions {
                let session_hit = fuzzy::score(query, &session.label).is_some();
                let tabs: Vec<FilterMatch> = model
                    .views
                    .get(&session.session_id)
                    .map(|view| {
                        view.windows
                            .iter()
                            .map(|w| (w.id().to_owned(), titles::tab_title(w, &model)))
                            .filter(|(_, title)| {
                                session_hit || fuzzy::score(query, title).is_some()
                            })
                            .map(|(id, title)| FilterMatch {
                                host,
                                session: session.session_id.clone(),
                                window: Some(id),
                                title,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if session_hit || !tabs.is_empty() {
                    matches.push(FilterMatch {
                        host,
                        session: session.session_id.clone(),
                        window: None,
                        title: session.label.clone(),
                    });
                    matches.extend(tabs);
                }
            }
        }
        matches
    }

    /// The titles the filter lists, in order, for tests and the debug report.
    pub fn filter_titles(&self) -> Vec<String> {
        self.filter_matches().into_iter().map(|m| m.title).collect()
    }

    pub fn filter_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        gpui::Focusable::focus_handle(self.filter.input.read(cx), cx).is_focused(window)
    }

    /// `client.sidebar.filter.focus`: show the sessions sidebar and type into its filter.
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interface = &mut self.settings.interface;
        if !interface.sidebar || interface.activity_view {
            interface.sidebar = true;
            interface.activity_view = false;
            self.save_settings();
        }
        let handle = gpui::Focusable::focus_handle(self.filter.input.read(cx), cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn on_filter_input(&mut self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Changed(text) => {
                self.filter.query = text.clone();
                // The best match is usually a tab, so start there when there is one.
                let matches = self.filter_matches();
                self.filter.selected = matches.iter().position(|m| m.window.is_some()).unwrap_or(0);
            }
            InputEvent::Navigate(step) => {
                let count = self.filter_matches().len();
                if count > 0 {
                    let next = self.filter.selected as i64 + *step as i64;
                    self.filter.selected = next.rem_euclid(count as i64) as usize;
                }
            }
            InputEvent::Submit { .. } => {
                if let Some(chosen) = self.filter_matches().get(self.filter.selected).cloned() {
                    self.show_match(chosen, window, cx);
                }
            }
            InputEvent::Cancel => self.clear_filter(window, cx),
            InputEvent::Tab => {}
        }
        cx.notify();
    }

    /// Empty the filter and go back to the focused pane.
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.query.clear();
        self.filter.selected = 0;
        self.filter
            .input
            .update(cx, |input, cx| input.set_text("", cx));
        self.refocus(window, cx);
        cx.notify();
    }

    fn show_match(&mut self, chosen: FilterMatch, window: &mut Window, cx: &mut Context<Self>) {
        self.select_session(chosen.host, chosen.session.clone(), cx);
        if let Some(id) = chosen.window {
            self.focus_tab(chosen.host, chosen.session.clone(), id);
        }
        self.clear_filter(window, cx);
    }

    pub(super) fn render_filter_field(&self) -> AnyElement {
        div()
            .px(px(2.))
            .pb(px(4.))
            .child(self.filter.input.clone())
            .into_any_element()
    }

    pub(super) fn render_filter_results(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let matches = self.filter_matches();
        let mut list = div().flex().flex_col().gap(px(2.));
        let mut last_host = None;
        for (index, found) in matches.iter().enumerate() {
            if last_host != Some(found.host) {
                list = list.child(heading(self.hosts[found.host].config.label.clone(), theme));
                last_host = Some(found.host);
            }
            let selected = index == self.filter.selected;
            let chosen = found.clone();
            let row = list_row(
                SharedString::from(format!("filter-{index}")),
                selected,
                theme,
            )
            .when(found.window.is_some(), |d| d.ml(px(18.)).h(px(28.)))
            .child(
                icon(if found.window.is_some() {
                    "terminal-2"
                } else {
                    "layout-columns"
                })
                .size(px(14.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .child(found.title.clone()),
            )
            .on_click(
                cx.listener(move |ws, _, window, cx| ws.show_match(chosen.clone(), window, cx)),
            );
            list = list.child(row);
        }
        if matches.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child("No tabs match"),
            );
        }
        list.into_any_element()
    }
}
