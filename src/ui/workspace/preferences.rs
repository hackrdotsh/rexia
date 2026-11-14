//! The settings page: rexia's preferences by section, applied and saved as they change.
//! Colors, corners, borders and fonts aren't here: they follow Noctalia.
use super::{Workspace, overlay::Overlay};
use crate::{
    rex::{activity::ProgramState, host::HostCommand},
    settings::{FadeInactive, PaneHeaders, PaneStyle, Settings, StateActivity},
    sound::{self, Cue},
    ui::{
        components::list_row,
        fonts,
        icons::icon,
        input::{InputEvent, TextInput},
        theme::Theme,
    },
};
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, Keystroke, SharedString, Stateful, Subscription,
    Window, div, point, prelude::*, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Interface,
    Terminal,
    Notifications,
    Hosts,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::Interface,
        Section::Terminal,
        Section::Notifications,
        Section::Hosts,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Interface => "Interface",
            Section::Terminal => "Terminal",
            Section::Notifications => "Notifications",
            Section::Hosts => "Client & Hosts",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Interface => "layout-sidebar",
            Section::Terminal => "terminal-2",
            Section::Notifications => "bell",
            Section::Hosts => "server",
        }
    }
}

pub struct SettingsPage {
    pub section: Section,
    label: Entity<TextInput>,
    font: Entity<TextInput>,
    _subscriptions: [Subscription; 2],
}

fn state_mut(settings: &mut Settings, state: ProgramState) -> &mut StateActivity {
    let n = &mut settings.notifications;
    match state {
        ProgramState::Blocked => &mut n.blocked,
        ProgramState::Error => &mut n.error,
        ProgramState::Done => &mut n.done,
        _ => &mut n.working,
    }
}

/// An on/off switch drawn with Noctalia's corners and borders.
fn switch(on: bool, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .when(on, |d| d.justify_end())
        .w(px(36.))
        .h(px(20.))
        .p(px(2.))
        .rounded(theme.radius_large())
        .when(theme.input_borders, |d| {
            d.border(theme.border_width)
                .border_color(if on { theme.primary } else { theme.border })
        })
        .bg(if on {
            theme.primary
        } else {
            theme.surface_high
        })
        .child(div().size(px(14.)).rounded(theme.radius_large()).bg(if on {
            theme.on_primary
        } else {
            theme.text_muted
        }))
}

fn row(id: impl Into<SharedString>, title: &str, detail: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex()
        .items_center()
        .gap(px(16.))
        .py(px(10.))
        .border_b(theme.border_width)
        .border_color(theme.overlay(0.08))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(14.)).child(title.to_owned()))
                .when(!detail.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.text_muted)
                            .child(detail.to_owned()),
                    )
                }),
        )
}

fn button(id: impl Into<SharedString>, label: &str, filled: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(28.))
        .min_w(px(28.))
        .px(px(10.))
        .rounded(theme.radius_small())
        .text_size(px(13.))
        .cursor_pointer()
        .when(filled, |d| d.bg(theme.primary).text_color(theme.on_primary))
        .when(!filled, |d| {
            d.text_color(theme.text)
                .hover(|d| d.bg(theme.overlay(0.08)))
                .when(theme.button_borders, |d| {
                    d.border(theme.border_width).border_color(theme.border)
                })
        })
        .child(label.to_owned())
}

impl Workspace {
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = cx.new(|cx| TextInput::new(crate::settings::hostname(), cx));
        if let Some(text) = self.settings.client_label.clone() {
            label.update(cx, |input, cx| input.set_text(text, cx));
        }
        let font = cx.new(|cx| TextInput::new(fonts::default_terminal_font().family, cx));
        if let Some(text) = self.settings.terminal.font_family.clone() {
            font.update(cx, |input, cx| input.set_text(text, cx));
        }
        let on_submit = |ws: &mut Workspace,
                         _: &Entity<TextInput>,
                         event: &InputEvent,
                         _: &mut Window,
                         cx: &mut Context<Workspace>| {
            if let InputEvent::Submit { .. } = event {
                ws.commit_settings_fields(cx);
            }
        };
        let subscriptions = [
            cx.subscribe_in(&label, window, on_submit),
            cx.subscribe_in(&font, window, on_submit),
        ];
        // Keys go to the page, not the terminal underneath.
        window.focus(&self.focus, cx);
        self.overlay = Some(Overlay::Settings(SettingsPage {
            section: Section::Interface,
            label,
            font,
            _subscriptions: subscriptions,
        }));
        cx.notify();
    }

    /// The section the settings page shows, while it's open.
    pub fn settings_section(&self) -> Option<Section> {
        match &self.overlay {
            Some(Overlay::Settings(page)) => Some(page.section),
            _ => None,
        }
    }

    pub fn show_settings_section(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Overlay::Settings(page)) = &mut self.overlay else {
            return;
        };
        page.section = section;
        // A section with a field starts in it.
        let handle = match section {
            Section::Terminal => gpui::Focusable::focus_handle(page.font.read(cx), cx),
            Section::Hosts => gpui::Focusable::focus_handle(page.label.read(cx), cx),
            _ => self.focus.clone(),
        };
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Esc closes the page; Tab and Shift-Tab move between sections.
    pub(super) fn settings_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(current) = self.settings_section() else {
            return false;
        };
        match keystroke.key.as_str() {
            "escape" => self.close_overlay(window, cx),
            "tab" => {
                let index = Section::ALL.iter().position(|s| *s == current).unwrap_or(0);
                let step = if keystroke.modifiers.shift { 3 } else { 1 };
                self.show_settings_section(Section::ALL[(index + step) % 4], window, cx);
            }
            _ => return false,
        }
        true
    }

    /// Change settings, apply them to what's on screen, and save them.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.apply_settings(cx);
        self.save_settings();
        cx.notify();
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        self.font = super::terminal_font(&self.settings);
        let font = self.font.clone();
        let terminal = &self.settings.terminal;
        let padding = point(px(terminal.padding_x), px(terminal.padding_y));
        let copy_on_select = terminal.copy_on_select;
        for pane in self.panes.values() {
            pane.update(cx, |pane, cx| {
                pane.set_font(font.clone(), cx);
                pane.padding = padding;
                pane.copy_on_select = copy_on_select;
                cx.notify();
            });
        }
    }

    /// Apply what was typed into the page's fields: the client label and the font.
    pub(super) fn commit_settings_fields(&mut self, cx: &mut Context<Self>) {
        let Some(Overlay::Settings(page)) = &self.overlay else {
            return;
        };
        let text = |input: &Entity<TextInput>| {
            let text = input.read(cx).text().trim().to_owned();
            (!text.is_empty()).then_some(text)
        };
        let label = text(&page.label);
        let font = text(&page.font);
        if label != self.settings.client_label {
            for host in &self.hosts {
                host.handle.send(HostCommand::SetLabel(label.clone()));
            }
            self.identity.label = label.clone();
        }
        if label == self.settings.client_label && font == self.settings.terminal.font_family {
            return;
        }
        self.update_settings(
            move |s| {
                s.client_label = label;
                s.terminal.font_family = font;
            },
            cx,
        );
    }

    /// The terminal font the panes use.
    pub fn terminal_font_size(&self) -> f32 {
        self.font.size
    }

    #[allow(clippy::too_many_arguments)]
    fn toggle_row(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        get: fn(&Settings) -> bool,
        set: fn(&mut Settings, bool),
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = get(&self.settings);
        row(id, title, detail, theme)
            .cursor_pointer()
            .child(switch(on, theme))
            .on_click(cx.listener(move |ws, _, _, cx| ws.update_settings(|s| set(s, !on), cx)))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn choice_row<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        options: &[(T, &'static str)],
        current: T,
        set: fn(&mut Settings, T),
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let buttons = options.iter().enumerate().map(|(index, (value, label))| {
            let value = *value;
            button(format!("{id}-{index}"), label, value == current, theme).on_click(
                cx.listener(move |ws, _, _, cx| ws.update_settings(|s| set(s, value), cx)),
            )
        });
        row(id, title, detail, theme)
            .child(div().flex().flex_none().gap(px(4.)).children(buttons))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn stepper_row(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        value: String,
        down: fn(&mut Settings),
        up: fn(&mut Settings),
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        row(id, title, detail, theme)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        button(format!("{id}-down"), "−", false, theme).on_click(
                            cx.listener(move |ws, _, _, cx| ws.update_settings(down, cx)),
                        ),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .flex()
                            .justify_center()
                            .text_size(px(13.))
                            .child(value),
                    )
                    .child(
                        button(format!("{id}-up"), "+", false, theme)
                            .on_click(cx.listener(move |ws, _, _, cx| ws.update_settings(up, cx))),
                    ),
            )
            .into_any_element()
    }

    fn field_row(
        id: &'static str,
        title: &str,
        detail: &str,
        input: &Entity<TextInput>,
        theme: &Theme,
    ) -> AnyElement {
        row(id, title, detail, theme)
            .child(div().w(px(260.)).flex_none().child(input.clone()))
            .into_any_element()
    }

    fn interface_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let i = &self.settings.interface;
        vec![
            row(
                "appearance",
                "Appearance",
                "Colors, corners, borders and the interface font follow Noctalia, live.",
                theme,
            )
            .child(icon("palette").size(px(18.)).text_color(theme.primary))
            .into_any_element(),
            self.toggle_row(
                "sidebar",
                "Show sidebar",
                "Hosts and sessions, or program activity, beside the panes.",
                |s| s.interface.sidebar,
                |s, v| s.interface.sidebar = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "activity-view",
                "Show activity in the sidebar",
                "List programs that report status instead of sessions.",
                |s| s.interface.activity_view,
                |s, v| s.interface.activity_view = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "activity-priority",
                "Sort activity by priority",
                "Most urgent first, instead of grouped by session.",
                |s| s.interface.activity_by_priority,
                |s, v| s.interface.activity_by_priority = v,
                theme,
                cx,
            ),
            self.choice_row(
                "pane-style",
                "Pane style",
                "Spacing between panes.",
                &[
                    (PaneStyle::Comfortable, "Comfortable"),
                    (PaneStyle::Compact, "Compact"),
                ],
                i.pane_style,
                |s, v| s.interface.pane_style = v,
                theme,
                cx,
            ),
            self.choice_row(
                "pane-headers",
                "Pane headers",
                "The title and controls above each pane.",
                &[
                    (PaneHeaders::Always, "Always"),
                    (PaneHeaders::Hover, "On hover"),
                    (PaneHeaders::Hidden, "Hidden"),
                ],
                i.pane_headers,
                |s, v| s.interface.pane_headers = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "highlight",
                "Highlight active pane edges",
                "Draw the focused pane's border in the primary color.",
                |s| s.interface.highlight_active_pane,
                |s, v| s.interface.highlight_active_pane = v,
                theme,
                cx,
            ),
            self.choice_row(
                "fade",
                "Fade inactive panes",
                "Dim the headers, the text, or both of unfocused panes.",
                &[
                    (FadeInactive::Off, "Off"),
                    (FadeInactive::Surface, "Header"),
                    (FadeInactive::Content, "Text"),
                    (FadeInactive::Both, "Both"),
                ],
                i.fade_inactive,
                |s, v| s.interface.fade_inactive = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "focus-follows-mouse",
                "Focus follows mouse",
                "Focus a pane by moving the pointer over it.",
                |s| s.interface.focus_follows_mouse,
                |s, v| s.interface.focus_follows_mouse = v,
                theme,
                cx,
            ),
        ]
    }

    fn terminal_section(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = &self.settings.terminal;
        vec![
            Self::field_row(
                "font",
                "Terminal font",
                "Empty uses Ghostty's font or fontconfig's monospace. Enter applies.",
                &page.font,
                theme,
            ),
            self.stepper_row(
                "font-size",
                "Font size",
                "",
                format!("{:.0} pt", self.font.size),
                |s| {
                    let size = s
                        .terminal
                        .font_size
                        .unwrap_or(fonts::TerminalFont::DEFAULT_SIZE);
                    s.terminal.font_size = Some((size - 1.).max(6.));
                },
                |s| {
                    let size = s
                        .terminal
                        .font_size
                        .unwrap_or(fonts::TerminalFont::DEFAULT_SIZE);
                    s.terminal.font_size = Some((size + 1.).min(72.));
                },
                theme,
                cx,
            ),
            self.stepper_row(
                "line-height",
                "Line height",
                "Space between terminal lines.",
                format!("{:.2}×", t.line_height),
                |s| s.terminal.line_height = (s.terminal.line_height - 0.05).max(0.8),
                |s| s.terminal.line_height = (s.terminal.line_height + 0.05).min(2.0),
                theme,
                cx,
            ),
            self.stepper_row(
                "padding-x",
                "Padding X",
                "Space left and right of the terminal text.",
                format!("{:.0} px", t.padding_x),
                |s| s.terminal.padding_x = (s.terminal.padding_x - 1.).max(0.),
                |s| s.terminal.padding_x = (s.terminal.padding_x + 1.).min(48.),
                theme,
                cx,
            ),
            self.stepper_row(
                "padding-y",
                "Padding Y",
                "Space above and below the terminal text.",
                format!("{:.0} px", t.padding_y),
                |s| s.terminal.padding_y = (s.terminal.padding_y - 1.).max(0.),
                |s| s.terminal.padding_y = (s.terminal.padding_y + 1.).min(48.),
                theme,
                cx,
            ),
            self.toggle_row(
                "copy-on-select",
                "Copy on select",
                "Copy selected text to the clipboard as soon as it's selected.",
                |s| s.terminal.copy_on_select,
                |s, v| s.terminal.copy_on_select = v,
                theme,
                cx,
            ),
        ]
    }

    fn notifications_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let n = &self.settings.notifications;
        let mut rows = vec![
            self.toggle_row(
                "activity-tabs",
                "Show activity in tabs",
                "Working dots and attention badges on tabs, sessions and panes.",
                |s| s.notifications.activity_in_tabs,
                |s, v| s.notifications.activity_in_tabs = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "include-idle",
                "Include idle processes in activity",
                "Keep programs that report idle in the activity list.",
                |s| s.notifications.include_idle,
                |s, v| s.notifications.include_idle = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "each-process",
                "Show each process",
                "List every reporting pane of a tab on its own row.",
                |s| s.notifications.each_process,
                |s, v| s.notifications.each_process = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "system-notifications",
                "Show desktop notifications",
                "Notify through the desktop when a pane out of view needs you.",
                |s| s.notifications.system_notifications,
                |s, v| s.notifications.system_notifications = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "terminal-notifications",
                "Terminal notifications",
                "Let programs send desktop notifications with OSC 9 and OSC 777.",
                |s| s.notifications.terminal_notifications,
                |s, v| s.notifications.terminal_notifications = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "badge",
                "Badge launcher icon",
                "Show how many panes need you on rexia's icon, in docks that support it.",
                |s| s.notifications.badge,
                |s, v| s.notifications.badge = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "sounds",
                "Play sounds",
                "Sounds from the desktop's sound theme when a program needs you.",
                |s| s.notifications.sounds,
                |s, v| s.notifications.sounds = v,
                theme,
                cx,
            ),
        ];
        let volume = n.sound_volume;
        rows.push(
            row("volume", "Sound volume", "", theme)
                .child(
                    button("volume-test", "Test", false, theme).on_click(move |_, _, _| {
                        sound::play(Cue::Done, volume);
                    }),
                )
                .child(
                    button("volume-down", "−", false, theme).on_click(cx.listener(
                        |ws, _, _, cx| {
                            ws.update_settings(
                                |s| {
                                    s.notifications.sound_volume =
                                        ((s.notifications.sound_volume - 0.1) * 10.).round() / 10.;
                                    s.notifications.sound_volume =
                                        s.notifications.sound_volume.max(0.);
                                },
                                cx,
                            )
                        },
                    )),
                )
                .child(
                    div()
                        .w(px(48.))
                        .flex()
                        .justify_center()
                        .text_size(px(13.))
                        .child(format!("{:.0}%", volume * 100.)),
                )
                .child(button("volume-up", "+", false, theme).on_click(cx.listener(
                    |ws, _, _, cx| {
                        ws.update_settings(
                            |s| {
                                s.notifications.sound_volume =
                                    ((s.notifications.sound_volume + 0.1) * 10.).round() / 10.;
                                s.notifications.sound_volume = s.notifications.sound_volume.min(1.);
                            },
                            cx,
                        )
                    },
                )))
                .into_any_element(),
        );
        // What each reported state does: its tab mark, a notification, a sound.
        let column = |text: &'static str| {
            div()
                .w(px(56.))
                .flex()
                .justify_center()
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(text)
        };
        rows.push(
            div()
                .flex()
                .items_center()
                .pt(px(16.))
                .pb(px(4.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(theme.text_muted)
                        .child("Per state"),
                )
                .child(column("Tabs"))
                .child(column("Notify"))
                .child(column("Sound"))
                .into_any_element(),
        );
        for (state, title) in [
            (ProgramState::Working, "Working"),
            (ProgramState::Blocked, "Blocked"),
            (ProgramState::Error, "Error"),
            (ProgramState::Done, "Done"),
        ] {
            let current = n.for_state(state);
            let cell = |field: usize, on: bool| {
                div()
                    .id(SharedString::from(format!("state-{title}-{field}")))
                    .w(px(56.))
                    .flex()
                    .justify_center()
                    .cursor_pointer()
                    .child(switch(on, theme))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.update_settings(
                            |s| {
                                let a = state_mut(s, state);
                                match field {
                                    0 => a.tabs = !a.tabs,
                                    1 => a.notify = !a.notify,
                                    _ => a.sound = !a.sound,
                                }
                            },
                            cx,
                        )
                    }))
            };
            rows.push(
                row(format!("state-{title}"), title, "", theme)
                    .child(cell(0, current.tabs))
                    .child(cell(1, current.notify))
                    .child(cell(2, current.sound))
                    .into_any_element(),
            );
        }
        rows
    }

    fn hosts_section(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows = vec![Self::field_row(
            "client-label",
            "Client label",
            "Names this computer in `rex client ls`. Enter applies.",
            &page.label,
            theme,
        )];
        rows.push(
            div()
                .flex()
                .items_center()
                .pt(px(16.))
                .pb(px(4.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(theme.text_muted)
                        .child("Hosts, kept on the home server"),
                )
                .child(
                    button("host-add", "Add Host…", true, theme).on_click(cx.listener(
                        |ws, _, window, cx| {
                            ws.commit_settings_fields(cx);
                            ws.open_add_host(window, cx);
                        },
                    )),
                )
                .into_any_element(),
        );
        for (index, host) in self.hosts.iter().enumerate() {
            let model = host.handle.snapshot();
            let status = match &model.connection {
                crate::rex::host::Connection::Connected => "Connected".to_owned(),
                crate::rex::host::Connection::Connecting => "Connecting…".to_owned(),
                crate::rex::host::Connection::Disconnected(reason) => reason.clone(),
            };
            let endpoint = if host.config.is_local() {
                "This computer".to_owned()
            } else {
                host.config.endpoint.clone()
            };
            let detail = format!("{endpoint} · {status}");
            let mut line = row(format!("host-{index}"), &host.config.label, &detail, theme)
                .child(icon("server").size(px(16.)).text_color(theme.text_muted));
            if let Some(id) = host.server_id.clone() {
                let label = host.config.label.clone();
                line = line.child(
                    button(format!("host-remove-{index}"), "Remove", false, theme).on_click(
                        cx.listener(move |ws, _, window, cx| {
                            ws.commit_settings_fields(cx);
                            ws.open_confirm(
                                format!("Remove {label}?"),
                                "Every device that uses this home server stops listing it. Its sessions keep running.".into(),
                                "Remove Host".into(),
                                super::Confirm::RemoveHost { id: id.clone() },
                                cx,
                            );
                            window.focus(&ws.focus, cx);
                        }),
                    ),
                );
            }
            rows.push(line.into_any_element());
        }
        rows
    }

    pub(super) fn render_settings(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = match page.section {
            Section::Interface => self.interface_section(theme, cx),
            Section::Terminal => self.terminal_section(page, theme, cx),
            Section::Notifications => self.notifications_section(theme, cx),
            Section::Hosts => self.hosts_section(page, theme, cx),
        };
        let nav: Vec<_> = Section::ALL
            .iter()
            .map(|section| {
                let section = *section;
                list_row(
                    SharedString::from(format!("settings-{}", section.title())),
                    section == page.section,
                    theme,
                )
                .child(icon(section.icon()).size(px(16.)))
                .child(div().text_size(px(13.)).child(section.title()))
                .on_click(cx.listener(move |ws, _, window, cx| {
                    ws.show_settings_section(section, window, cx)
                }))
            })
            .collect();
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(px(200.))
                    .h_full()
                    .p(px(10.))
                    .gap(px(2.))
                    .border_r(theme.border_width)
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(6.))
                            .pb(px(10.))
                            .child(icon("settings").size(px(20.)).text_color(theme.primary))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Settings"),
                            ),
                    )
                    .children(nav),
            )
            .child(
                div()
                    .id("settings-content")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px(px(20.))
                    .py(px(14.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .pb(px(6.))
                            .child(page.section.title()),
                    )
                    .children(rows),
            )
            .into_any_element()
    }
}
