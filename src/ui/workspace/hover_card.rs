//! Hover cards: resting the pointer on a tab shows each of its panes' program, activity
//! and directory, and on a pane header, that pane's. A pane in a tab's card is a button
//! that goes to it. Cards follow the panes live while they're open.
use super::{Workspace, state_color};
use crate::{
    rex::{activity::ProgramState, host::HostModel},
    ui::{icons::icon, theme::Theme, titles},
};
use gpui::{
    AnyView, App, Bounds, Context, Entity, Hsla, Pixels, SharedString, Subscription, WeakEntity,
    Window, canvas, div, prelude::*, px, relative,
};
use serde_json::json;

pub(super) const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(650);

/// What the card is about.
#[derive(Clone, Debug, PartialEq)]
pub enum CardTarget {
    /// A tab, by its window id: every pane in it.
    Tab(String),
    /// One pane, by its block id.
    Pane(String),
}

/// The card shown last, for tests: its text, line by line, and where each pane's row is.
#[derive(Default)]
pub struct ShownCard {
    pub target: Option<CardTarget>,
    pub lines: Vec<String>,
    pub rows: Vec<(String, Bounds<Pixels>)>,
}

impl gpui::Global for ShownCard {}

pub struct HoverCard {
    workspace: WeakEntity<Workspace>,
    target: CardTarget,
    _observe: Option<Subscription>,
}

/// One pane in a card.
struct PaneInfo {
    block_id: String,
    title: String,
    icon: &'static str,
    colors: Option<(u32, u32)>,
    focused: bool,
    directory: Option<String>,
    /// What each program reporting in it says: who, its state, its words, and progress.
    reports: Vec<Report>,
    /// A terminal progress bar (OSC 9;4) with no program report.
    progress: Option<u8>,
}

struct Report {
    app: Option<String>,
    state: ProgramState,
    badge: &'static str,
    message: Option<String>,
    progress: Option<u8>,
}

fn home_relative(path: &str, local: bool) -> String {
    let path = path.strip_prefix("file://").map_or(path, |rest| {
        // file://HOST/path: keep the path.
        rest.find('/').map_or(rest, |i| &rest[i..])
    });
    if local
        && let Some(home) = std::env::var_os("HOME")
        && let Some(home) = home.to_str()
        && !home.is_empty()
        && let Some(rest) = path.strip_prefix(home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_owned()
}

fn panes_of(target: &CardTarget, model: &HostModel, local: bool) -> (String, Vec<PaneInfo>) {
    let Some(view) = model.view() else {
        return (String::new(), Vec::new());
    };
    let window = match target {
        CardTarget::Tab(id) => view.window(id),
        CardTarget::Pane(block) => view.window_of(block),
    };
    let Some(window) = window else {
        return (String::new(), Vec::new());
    };
    let heading = match target {
        CardTarget::Tab(_) => titles::tab_title(window, model),
        CardTarget::Pane(_) => String::new(),
    };
    let panes = window
        .blocks()
        .filter_map(|block| {
            let id = block.block_id.clone()?;
            if let CardTarget::Pane(only) = target
                && *only != id
            {
                return None;
            }
            let state = model.blocks.get(&id);
            let reports: Vec<Report> = state
                .map(|s| {
                    let mut records: Vec<_> = s.records.values().collect();
                    records.sort_by_key(|r| std::cmp::Reverse(r.state));
                    records
                        .into_iter()
                        .map(|r| Report {
                            app: r.app.clone(),
                            state: r.state,
                            badge: r.badge(),
                            message: r.msg.clone().or_else(|| r.title.clone()),
                            progress: r.progress,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let title = titles::pane_title(block, state);
            Some(PaneInfo {
                icon: titles::program_icon(state),
                colors: crate::ui::program_icons::pane_colors(model, &id),
                focused: window.focused_block_id.as_deref() == Some(id.as_str()),
                directory: state
                    .and_then(|s| s.pwd.as_deref().or(s.process.cwd.as_deref()))
                    .map(|p| home_relative(p, local))
                    .filter(|d| *d != title),
                progress: state
                    .and_then(|s| s.progress.as_ref())
                    .filter(|_| reports.is_empty())
                    .and_then(|(_, p)| *p),
                reports,
                title,
                block_id: id,
            })
        })
        .collect();
    (heading, panes)
}

fn progress_bar(percent: u8, color: Hsla, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(theme.overlay(0.12))
        .child(
            div()
                .h_full()
                .w(relative(percent as f32 / 100.))
                .rounded_full()
                .bg(color),
        )
}

impl HoverCard {
    fn go_to(&self, block: &str, cx: &mut App) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        workspace.update(cx, |ws, cx| ws.go_to_pane(block.to_owned(), cx));
    }
}

impl Render for HoverCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let (model, local, obscures_interaction) = {
            let ws = workspace.read(cx);
            (
                ws.model(),
                ws.host().handle.endpoint.is_local(),
                ws.context_menu_open() || ws.dragging().is_some(),
            )
        };
        // Keep cards out of the way of context menus and drag targets.
        if obscures_interaction {
            if cx
                .try_global::<ShownCard>()
                .is_some_and(|s| s.target.is_some())
            {
                cx.set_global(ShownCard::default());
            }
            return div().into_any_element();
        }
        let (heading, panes) = panes_of(&self.target, &model, local);
        let tab = matches!(self.target, CardTarget::Tab(_));

        // What the card says, for tests.
        let mut lines = Vec::new();
        if !heading.is_empty() {
            lines.push(heading.clone());
        }
        for pane in &panes {
            lines.push(pane.title.clone());
            for report in &pane.reports {
                let mut line = match &report.app {
                    Some(app) => format!("{app}: {}", report.badge),
                    None => report.badge.to_owned(),
                };
                if let Some(message) = &report.message {
                    line.push_str(&format!(" · {message}"));
                }
                if let Some(progress) = report.progress {
                    line.push_str(&format!(" · {progress}%"));
                }
                lines.push(line);
            }
            if let Some(progress) = pane.progress {
                lines.push(format!("{progress}%"));
            }
            if let Some(directory) = &pane.directory {
                lines.push(directory.clone());
            }
        }
        let target = self.target.clone();
        let shown = cx.try_global::<ShownCard>();
        if shown.is_none_or(|s| s.target.as_ref() != Some(&target) || s.lines != lines) {
            let rows = shown
                .filter(|s| s.target.as_ref() == Some(&target))
                .map(|s| s.rows.clone())
                .unwrap_or_default();
            cx.set_global(ShownCard {
                target: Some(target),
                lines,
                rows,
            });
        }

        let mut card = div()
            .id("hover-card")
            .occlude()
            .w(px(320.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(6.))
            .font_family(theme.ui_font.clone())
            .text_size(px(12.))
            .text_color(theme.text)
            .bg(theme.surface_high)
            .rounded(theme.radius())
            .when(theme.popup_borders, |d| {
                d.border(theme.border_width).border_color(theme.border)
            })
            .when(theme.popup_shadows, |d| d.shadow_lg())
            .when(!heading.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .px(px(6.))
                        .pt(px(2.))
                        .pb(px(4.))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(heading.clone()),
                        )
                        .child(div().flex_none().text_color(theme.text_muted).child(
                            match panes.len() {
                                1 => "1 pane".to_owned(),
                                n => format!("{n} panes"),
                            },
                        )),
                )
            });
        for pane in panes {
            let block = pane.block_id.clone();
            let entity: Entity<Self> = cx.entity();
            let recorded = block.clone();
            let tracker = canvas(
                move |bounds, _, cx| {
                    let card = entity.read(cx).target.clone();
                    if let Some(shown) = cx.try_global::<ShownCard>()
                        && shown.target.as_ref() == Some(&card)
                    {
                        let mut rows = shown.rows.clone();
                        rows.retain(|(id, _)| *id != recorded);
                        rows.push((recorded.clone(), bounds));
                        cx.global_mut::<ShownCard>().rows = rows;
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full();
            let mut row = div()
                .id(SharedString::from(format!("card-pane-{block}")))
                .relative()
                .flex()
                .flex_col()
                .gap(px(3.))
                .px(px(6.))
                .py(px(5.))
                .rounded(theme.radius_small())
                .child(tracker)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(crate::ui::program_icons::render_icon(
                            pane.icon,
                            18.,
                            theme.text,
                            &theme,
                            pane.colors,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(13.))
                                .child(pane.title.clone()),
                        )
                        .when(tab && pane.focused, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_muted)
                                    .child("Focused"),
                            )
                        }),
                );
            for report in &pane.reports {
                let state = state_color(report.state, &theme);
                row = row.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .pl(px(22.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().flex_none().size(px(6.)).rounded_full().bg(state))
                                .when_some(report.app.clone(), |d, app| {
                                    d.child(div().flex_none().child(app))
                                })
                                .child(div().flex_none().text_color(state).child(report.badge)),
                        )
                        .when_some(report.message.clone(), |d, message| {
                            d.child(div().text_color(theme.text_muted).child(message))
                        })
                        .when_some(report.progress, |d, p| {
                            d.child(progress_bar(p, state, &theme))
                        }),
                );
            }
            if let Some(progress) = pane.progress {
                row = row.child(div().pl(px(22.)).child(progress_bar(
                    progress,
                    theme.primary,
                    &theme,
                )));
            }
            if let Some(directory) = pane.directory.clone() {
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(22.))
                        .text_color(theme.text_muted)
                        .child(icon("folder").size(px(12.)).text_color(theme.text_muted))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(directory),
                        ),
                );
            }
            if tab {
                row = row
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.overlay(0.08)))
                    .on_click(cx.listener(move |card, _, _, cx| card.go_to(&block, cx)));
            }
            card = card.child(row);
        }
        div().pl(px(8.)).pt(px(8.)).child(card).into_any_element()
    }
}

impl Workspace {
    /// A hover card for a tab or pane, for `.hoverable_tooltip(...)`.
    pub(super) fn hover_card(
        &self,
        target: CardTarget,
        cx: &Context<Self>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let workspace = cx.entity().downgrade();
        move |_, cx| {
            let target = target.clone();
            let workspace = workspace.clone();
            cx.new(|cx| {
                let observe = workspace
                    .upgrade()
                    .map(|ws| cx.observe(&ws, |_, _, cx| cx.notify()));
                HoverCard {
                    workspace,
                    target,
                    _observe: observe,
                }
            })
            .into()
        }
    }

    /// Show a pane, switching to its tab: what clicking it in a hover card does.
    pub fn go_to_pane(&mut self, block: String, cx: &mut Context<Self>) {
        if let Some(session) = self.session_id() {
            self.call(
                "session.focus_block",
                json!({"session_id": session, "block_id": block}),
            );
        }
        cx.notify();
    }
}
