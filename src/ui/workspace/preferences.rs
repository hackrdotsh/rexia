//! The settings page: rexia's preferences by section, applied and saved as they change.
//! Colors, corners, borders and fonts aren't here: they follow Noctalia.
use super::{Workspace, overlay::Overlay};
use crate::settings::{
    AnimationStyle, AppLinks, SelectionColor, ShellIconTheme, SidebarGesture, SidebarTextSize,
    TabIconColor,
};
use crate::{
    rex::stream::CursorShape,
    rex::{activity::ProgramState, host::HostCommand},
    settings::{
        BlinkSpeed, FadeInactive, HighlightAnimation, HighlightColor, NoticePosition, PaddingFill,
        PaneHeaders, PaneStyle, Settings, StateActivity, TextStyles, ToolbarBehavior,
        ToolbarLayout,
    },
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
    Server,
    Integrations,
    Developer,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Interface,
        Section::Terminal,
        Section::Notifications,
        Section::Hosts,
        Section::Server,
        Section::Integrations,
        Section::Developer,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Interface => "Interface",
            Section::Terminal => "Terminal",
            Section::Notifications => "Notifications",
            Section::Hosts => "Client & Hosts",
            Section::Server => "Server",
            Section::Integrations => "Integrations",
            Section::Developer => "Developer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Interface => "layout-sidebar",
            Section::Terminal => "terminal-2",
            Section::Notifications => "bell",
            Section::Hosts => "server",
            Section::Server => "power",
            Section::Integrations => "command",
            Section::Developer => "file-code",
        }
    }
}

pub struct SettingsPage {
    pub section: Section,
    label: Entity<TextInput>,
    font: Entity<TextInput>,
    fallbacks: Entity<TextInput>,
    /// Tailscale's preferred hostname, tags and control server, read when signing in.
    pub(super) ts_hostname: Entity<TextInput>,
    pub(super) ts_tags: Entity<TextInput>,
    pub(super) ts_control: Entity<TextInput>,
    /// A custom SSH agent socket; Enter uses it.
    pub(super) ssh_socket: Entity<TextInput>,
    /// The startup command for new local terminals; Enter applies.
    pub(super) startup: Entity<TextInput>,
    /// A browser origin pattern to allow; Enter adds it.
    pub(super) origin: Entity<TextInput>,
    /// Where a rex of your own is; Enter uses it.
    pub(super) rex_source: Entity<TextInput>,
    _subscriptions: [Subscription; 7],
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

pub(super) fn row(
    id: impl Into<SharedString>,
    title: &str,
    detail: &str,
    theme: &Theme,
) -> Stateful<Div> {
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

/// A muted heading between groups of rows.
fn section_label(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .pt(px(16.))
        .pb(px(4.))
        .text_size(px(12.))
        .text_color(theme.text_muted)
        .child(text)
        .into_any_element()
}

pub(super) fn button(
    id: impl Into<SharedString>,
    label: &str,
    filled: bool,
    theme: &Theme,
) -> Stateful<Div> {
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
        let fallbacks = cx.new(|cx| TextInput::new("Symbols Nerd Font, Noto Color Emoji", cx));
        if !self.settings.terminal.fallback_fonts.is_empty() {
            let text = self.settings.terminal.fallback_fonts.join(", ");
            fallbacks.update(cx, |input, cx| input.set_text(text, cx));
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
        let startup = cx.new(|cx| TextInput::new("Default login shell", cx));
        if !self.settings.terminal.startup_command.is_empty() {
            let text = self.settings.terminal.startup_command.clone();
            startup.update(cx, |input, cx| input.set_text(text, cx));
        }
        let origin = cx.new(|cx| TextInput::new("https://*.example.com", cx));
        let rex_source = cx.new(|cx| TextInput::new("~/src/rex", cx));
        if let Some(location) = &self.settings.rex_location {
            let text = location.display().to_string();
            rex_source.update(cx, |input, cx| input.set_text(text, cx));
        }
        let ssh_socket = cx.new(|cx| TextInput::new("/path/to/agent.sock", cx));
        if let Some(socket) = &self.settings.ssh_agent {
            let text = socket.display().to_string();
            ssh_socket.update(cx, |input, cx| input.set_text(text, cx));
        }
        let subscriptions = [
            cx.subscribe_in(&label, window, on_submit),
            cx.subscribe_in(&font, window, on_submit),
            cx.subscribe_in(&fallbacks, window, on_submit),
            cx.subscribe_in(
                &rex_source,
                window,
                |ws: &mut Workspace, _: &Entity<TextInput>, event: &InputEvent, _, cx| {
                    if let InputEvent::Submit { .. } = event
                        && let Some(text) = ws.rex_field_text(cx)
                    {
                        ws.choose_rex(Some(text.into()), cx);
                    }
                },
            ),
            cx.subscribe_in(
                &origin,
                window,
                |ws: &mut Workspace, input: &Entity<TextInput>, event: &InputEvent, _, cx| {
                    if let InputEvent::Submit { .. } = event {
                        let text = input.read(cx).text().to_owned();
                        match ws.add_origin(&text, cx) {
                            Ok(()) => {
                                ws.origin_error = None;
                                input.update(cx, |input, cx| input.set_text(String::new(), cx));
                            }
                            Err(why) => ws.origin_error = Some(why),
                        }
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &startup,
                window,
                |ws: &mut Workspace, input: &Entity<TextInput>, event: &InputEvent, _, cx| {
                    if let InputEvent::Submit { .. } = event {
                        let text = input.read(cx).text().trim().to_owned();
                        ws.update_settings(|s| s.terminal.startup_command = text, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &ssh_socket,
                window,
                |ws: &mut Workspace, input: &Entity<TextInput>, event: &InputEvent, _, cx| {
                    if let InputEvent::Submit { .. } = event {
                        let text = input.read(cx).text().trim().to_owned();
                        ws.choose_ssh_agent((!text.is_empty()).then(|| text.into()), cx);
                    }
                },
            ),
        ];
        // Keys go to the page, not the terminal underneath.
        window.focus(&self.focus, cx);
        self.overlay = Some(Overlay::Settings(Box::new(SettingsPage {
            section: Section::Interface,
            label,
            font,
            fallbacks,
            ts_hostname: cx.new(|cx| TextInput::new("rex", cx)),
            ts_tags: cx.new(|cx| TextInput::new("tag:rex", cx)),
            ts_control: cx.new(|cx| TextInput::new("Tailscale's own", cx)),
            ssh_socket,
            startup,
            origin,
            rex_source,
            _subscriptions: subscriptions,
        })));
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
        // The Server section asks the server about itself each time it opens, and the
        // hosts section asks the SSH agent how it is.
        if section == Section::Server {
            self.refresh_server_report(cx);
        }
        if section == Section::Hosts {
            self.refresh_ssh_agent_status(cx);
        }
        if section == Section::Integrations {
            self.refresh_agents(cx);
        }
        if section == Section::Developer {
            self.check_rex(cx);
        }
        let Some(Overlay::Settings(page)) = &mut self.overlay else {
            return;
        };
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

    pub(super) fn apply_settings(&mut self, cx: &mut Context<Self>) {
        crate::rex::ssh_agent::choose(self.settings.ssh_agent.clone());
        super::apply_global_settings(&self.settings);
        self.font = super::terminal_font(&self.settings);
        let font = self.font.clone();
        let terminal = &self.settings.terminal;
        let padding = point(px(terminal.padding_x), px(terminal.padding_y));
        let copy_on_select = terminal.copy_on_select;
        let force_cursor = terminal.force_cursor_style.then_some(terminal.cursor_style);
        let blink = terminal.blink_interval();
        let cursor = terminal.cursor_style;
        let padding_fill = terminal.padding_fill;
        let smooth_scrolling = terminal.smooth_scrolling;
        let smooth_resizing = terminal.smooth_resizing;
        let anchor = terminal.anchor;
        let selection = terminal.selection;
        let cursor_shader = terminal.cursor_shaders.then_some(terminal.cursor_shader);
        let colors = crate::ui::terminal::TextColors {
            improve_contrast: terminal.improve_contrast,
            theme_palette: terminal.theme_palette,
        };
        for pane in self.panes.values() {
            pane.update(cx, |pane, cx| {
                pane.set_font(font.clone(), cx);
                pane.padding = padding;
                pane.copy_on_select = copy_on_select;
                pane.force_cursor = force_cursor;
                pane.blink = blink;
                pane.colors = colors;
                pane.padding_fill = padding_fill;
                pane.smooth_scrolling = smooth_scrolling;
                pane.smooth_resizing = smooth_resizing;
                pane.anchor = anchor;
                pane.selection = selection;
                pane.cursor_shader = cursor_shader;
                cx.notify();
            });
        }
        for host in &self.hosts {
            host.handle
                .send(crate::rex::host::HostCommand::DefaultCursor(Some(cursor)));
        }
    }

    /// The focused pane's cursor as drawn: shape, whether it blinks, whether it shows.
    pub fn focused_cursor(
        &self,
        cx: &gpui::App,
    ) -> Option<(crate::rex::stream::CursorShape, bool, bool)> {
        let block = self.focused.as_ref()?;
        self.panes.get(block)?.read(cx).cursor_state()
    }

    /// Act as an input method on the focused pane: compose `text`, or commit it with
    /// `commit`. Returns where the method would put its candidate window.
    pub fn input_method(
        &mut self,
        text: &str,
        commit: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        use gpui::EntityInputHandler;
        let pane = self
            .focused
            .as_ref()
            .and_then(|b| self.panes.get(b))?
            .clone();
        pane.update(cx, |pane, cx| {
            if commit {
                pane.replace_text_in_range(None, text, window, cx);
            } else {
                pane.replace_and_mark_text_in_range(None, text, None, window, cx);
            }
            pane.bounds_for_range(0..0, Default::default(), window, cx)
        })
    }

    /// What an input method is composing in the focused pane.
    pub fn focused_preedit(&self, cx: &gpui::App) -> Option<String> {
        let block = self.focused.as_ref()?;
        self.panes.get(block)?.read(cx).preedit().map(str::to_owned)
    }

    /// The color the focused pane draws a cell's text in.
    pub fn focused_text_color(&self, row: usize, col: usize, cx: &gpui::App) -> Option<u32> {
        let block = self.focused.as_ref()?;
        self.panes.get(block)?.read(cx).drawn_text_color(row, col)
    }

    /// The Kitty graphics drawn in the focused pane: image ID and area within the grid.
    pub fn focused_images(&self, cx: &gpui::App) -> Vec<(u32, gpui::Bounds<gpui::Pixels>)> {
        self.focused
            .as_ref()
            .and_then(|block| self.panes.get(block))
            .map(|pane| pane.read(cx).image_areas())
            .unwrap_or_default()
    }

    /// The program colors filling the focused pane's padding, and its header's color.
    pub fn focused_padding_fill(&self, cx: &gpui::App) -> Option<(Vec<u32>, Option<u32>)> {
        let block = self.focused.as_ref()?;
        let pane = self.panes.get(block)?.read(cx);
        Some((pane.padding_fill_colors(), pane.header_fill()))
    }

    /// Keep `block`'s pane from asking the server for a new size for `hold`, as moving
    /// panes do, for tests.
    pub fn hold_pane_size(&self, block: &str, hold: std::time::Duration, cx: &mut gpui::App) {
        if let Some(pane) = self.panes.get(block) {
            pane.update(cx, |pane, _| {
                pane.hold_size_until(std::time::Instant::now() + hold)
            });
        }
    }

    /// The Source directory field's text, with `~` expanded; `None` when it's empty.
    pub fn rex_field_text(&self, cx: &gpui::App) -> Option<String> {
        let Some(Overlay::Settings(page)) = &self.overlay else {
            return None;
        };
        let text = page.rex_source.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return None;
        }
        Some(match (text.strip_prefix("~/"), std::env::var("HOME")) {
            (Some(rest), Ok(home)) => format!("{home}/{rest}"),
            _ => text,
        })
    }

    /// The pane showing `block`, for tests.
    pub fn terminal_for(&self, block: &str) -> Option<Entity<crate::ui::terminal::TerminalPane>> {
        self.panes.get(block).cloned()
    }

    /// The focused pane, for tests.
    pub fn focused_terminal(&self) -> Option<Entity<crate::ui::terminal::TerminalPane>> {
        self.panes.get(self.focused.as_ref()?).cloned()
    }

    /// The focused pane's character cell width, in pixels.
    pub fn focused_cell_width(&self, cx: &gpui::App) -> Option<f32> {
        let block = self.focused.as_ref()?;
        let cell = self.panes.get(block)?.read(cx).cell_size()?;
        Some(f32::from(cell.width))
    }

    /// Step the focused pane's cursor blink, as its timer would.
    pub fn blink_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = self.focused.as_ref().and_then(|b| self.panes.get(b)) {
            pane.update(cx, |pane, cx| pane.blink_tick(cx));
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
        let fallbacks: Vec<String> = page
            .fallbacks
            .read(cx)
            .text()
            .split(',')
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty())
            .collect();
        if label != self.settings.client_label {
            for host in &self.hosts {
                host.handle.send(HostCommand::SetLabel(label.clone()));
            }
            self.identity.label = label.clone();
        }
        if label == self.settings.client_label
            && font == self.settings.terminal.font_family
            && fallbacks == self.settings.terminal.fallback_fonts
        {
            return;
        }
        self.update_settings(
            move |s| {
                s.client_label = label;
                s.terminal.font_family = font;
                s.terminal.fallback_fonts = fallbacks;
            },
            cx,
        );
    }

    /// The terminal font the panes use.
    pub fn terminal_font_size(&self) -> f32 {
        self.font.size
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn toggle_row(
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
    pub(super) fn choice_row<T: Copy + PartialEq + 'static>(
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

    pub(super) fn field_row(
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
            self.choice_row(
                "toolbar-layout",
                "Toolbar layout",
                "Tabs above the panes, or down the sidebar on either side.",
                &[
                    (ToolbarLayout::Above, "Above"),
                    (ToolbarLayout::Left, "Left side"),
                    (ToolbarLayout::Right, "Right side"),
                ],
                i.toolbar_layout,
                |s, v| {
                    s.interface.toolbar_layout = v;
                    if v.vertical() {
                        s.interface.vertical_side = v;
                        s.interface.sidebar = true;
                    }
                },
                theme,
                cx,
            ),
            self.choice_row(
                "toolbar-behavior",
                "Toolbar behavior",
                "Show on hover hides it until the pointer reaches the window's top edge.",
                &[
                    (ToolbarBehavior::Always, "Always"),
                    (ToolbarBehavior::Hover, "On hover"),
                    (ToolbarBehavior::Hidden, "Hidden"),
                ],
                i.toolbar_behavior,
                |s, v| s.interface.toolbar_behavior = v,
                theme,
                cx,
            ),
            self.choice_row(
                "animation-style",
                "Animation style",
                "How panes move, open and close, the active border travels, and the sidebar shows or hides.",
                &[
                    (AnimationStyle::Snappy, "Snappy"),
                    (AnimationStyle::Natural, "Natural"),
                    (AnimationStyle::Off, "Off"),
                ],
                i.animation,
                |s, v| s.interface.animation = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "tabs-fill",
                "Tabs fill available space",
                "Tabs above the panes share the tab bar equally.",
                |s| s.interface.tabs_fill,
                |s, v| s.interface.tabs_fill = v,
                theme,
                cx,
            ),
            self.choice_row(
                "tab-icon-color",
                "Use color for tab icons",
                "Which tab icons draw in their program's colors.",
                &[
                    (TabIconColor::Always, "Always"),
                    (TabIconColor::ActivePanes, "Active panes"),
                    (TabIconColor::ActiveTabs, "Active tabs"),
                    (TabIconColor::Never, "Never"),
                ],
                i.tab_icon_color,
                |s, v| s.interface.tab_icon_color = v,
                theme,
                cx,
            ),
            self.choice_row(
                "shell-icon-theme",
                "Shell icon theme",
                "A colored shell icon's own artwork, the terminal theme's colors, or the accent.",
                &[
                    (ShellIconTheme::Default, "Default"),
                    (ShellIconTheme::Theme, "Theme"),
                    (ShellIconTheme::Accent, "Accent"),
                ],
                i.shell_icon_theme,
                |s, v| s.interface.shell_icon_theme = v,
                theme,
                cx,
            ),
            self.stepper_row(
                "tab-icons",
                "Show tab icons",
                "Pane icons per tab: up to three in vertical tabs, five in horizontal tabs.",
                i.tab_icons.to_string(),
                |s| s.interface.tab_icons = s.interface.tab_icons.saturating_sub(1),
                |s| s.interface.tab_icons = (s.interface.tab_icons + 1).min(5),
                theme,
                cx,
            ),
            self.toggle_row(
                "sidebar",
                "Show sidebar",
                "Hosts and sessions, or program activity, beside the panes.",
                |s| s.interface.sidebar,
                |s, v| s.interface.sidebar = v,
                theme,
                cx,
            ),
            self.choice_row(
                "sidebar-text-size",
                "Sidebar text size",
                "The size of session and tab titles in the sidebar.",
                &[
                    (SidebarTextSize::Small, "Small"),
                    (SidebarTextSize::Medium, "Medium"),
                    (SidebarTextSize::Large, "Large"),
                ],
                i.sidebar_text_size,
                |s, v| s.interface.sidebar_text_size = v,
                theme,
                cx,
            ),
            self.choice_row(
                "sidebar-gesture",
                "Use a gesture to toggle the sidebar",
                "A two-finger swipe sideways shows or hides the sidebar; with Swipe, hold Alt to scroll sideways instead.",
                &[
                    (SidebarGesture::Swipe, "Swipe"),
                    (SidebarGesture::AltSwipe, "Alt-swipe"),
                    (SidebarGesture::Off, "Off"),
                ],
                i.sidebar_gesture,
                |s, v| s.interface.sidebar_gesture = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "trackpad-gestures",
                "Use trackpad gestures",
                "Spread two fingers over a pane to zoom it and pinch to restore it, and swipe for the sidebar.",
                |s| s.interface.trackpad_gestures,
                |s, v| s.interface.trackpad_gestures = v,
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
                "Draw the focused pane's border in the highlight color.",
                |s| s.interface.highlight_active_pane,
                |s, v| s.interface.highlight_active_pane = v,
                theme,
                cx,
            ),
            self.highlight_color_row(theme, cx),
            self.stepper_row(
                "highlight-opacity",
                "Highlight opacity",
                "How strongly the highlight draws.",
                format!("{:.0}%", i.highlight_opacity * 100.),
                |s| {
                    let v = ((s.interface.highlight_opacity - 0.1) * 10.).round() / 10.;
                    s.interface.highlight_opacity = v.max(0.2);
                },
                |s| {
                    let v = ((s.interface.highlight_opacity + 0.1) * 10.).round() / 10.;
                    s.interface.highlight_opacity = v.min(1.);
                },
                theme,
                cx,
            ),
            self.choice_row(
                "highlight-animation",
                "Animate active pane highlight",
                "Fade it in on the newly focused pane, move it there, or switch at once.",
                &[
                    (HighlightAnimation::Fade, "Fade"),
                    (HighlightAnimation::Move, "Move"),
                    (HighlightAnimation::Instant, "Instant"),
                ],
                i.highlight_animation,
                |s, v| s.interface.highlight_animation = v,
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
                "fade-inactive-windows",
                "Fade panes for inactive windows",
                "While the window isn't focused, its panes fade as inactive panes do.",
                |s| s.interface.fade_inactive_windows,
                |s, v| s.interface.fade_inactive_windows = v,
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
        let mut rows = vec![
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
            self.choice_row(
                "padding-fill",
                "Fill with program colors",
                "Let a full-screen program's background reach into the padding.",
                &[
                    (PaddingFill::Off, "Off"),
                    (PaddingFill::Automatic, "Automatic"),
                    (PaddingFill::Always, "Always"),
                ],
                t.padding_fill,
                |s, v| s.terminal.padding_fill = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "padding-fill-header",
                "Include pane header",
                "Color the pane header to match the program too, while the padding fills.",
                |s| s.terminal.padding_fill_header,
                |s, v| s.terminal.padding_fill_header = v,
                theme,
                cx,
            ),
            self.choice_row(
                "cursor-style",
                "Cursor style",
                "The cursor programs get until they choose their own.",
                &[
                    (CursorShape::Block, "Block"),
                    (CursorShape::Bar, "Bar"),
                    (CursorShape::Underline, "Underline"),
                ],
                t.cursor_style,
                |s, v| s.terminal.cursor_style = v,
                theme,
                cx,
            ),
            self.toggle_row(
                "force-cursor",
                "Force style",
                "Ignore programs that change the cursor's shape.",
                |s| s.terminal.force_cursor_style,
                |s, v| s.terminal.force_cursor_style = v,
                theme,
                cx,
            ),
            self.choice_row(
                "cursor-blink",
                "Blink speed",
                "How fast cursors blink when their program asks them to.",
                &[
                    (BlinkSpeed::Off, "Off"),
                    (BlinkSpeed::Slow, "Slow"),
                    (BlinkSpeed::Normal, "Normal"),
                    (BlinkSpeed::Fast, "Fast"),
                    (BlinkSpeed::Custom, "Custom"),
                ],
                t.cursor_blink,
                |s, v| s.terminal.cursor_blink = v,
                theme,
                cx,
            ),
        ];
        if t.cursor_blink == BlinkSpeed::Custom {
            rows.push(self.stepper_row(
                "blink-interval",
                "Blink interval",
                "The time between blinks.",
                format!("{} ms", t.cursor_blink_ms),
                |s| {
                    s.terminal.cursor_blink_ms =
                        s.terminal.cursor_blink_ms.saturating_sub(50).max(100)
                },
                |s| s.terminal.cursor_blink_ms = (s.terminal.cursor_blink_ms + 50).min(5000),
                theme,
                cx,
            ));
        }
        rows.extend(self.cursor_shader_rows(theme, cx));
        rows.push(self.toggle_row(
            "copy-on-select",
            "Copy on select",
            "Copy selected text to the clipboard as soon as it's selected.",
            |s| s.terminal.copy_on_select,
            |s, v| s.terminal.copy_on_select = v,
            theme,
            cx,
        ));
        let sel = t.selection;
        rows.push(self.selection_color_row(
            "selection-foreground",
            "Selection foreground",
            "The text color inside a selection.",
            sel.foreground,
            true,
            |s, v| s.terminal.selection.foreground = v,
            theme,
            cx,
        ));
        rows.push(self.selection_color_row(
            "selection-background",
            "Selection background",
            "The fill color behind selected text.",
            sel.background,
            false,
            |s, v| s.terminal.selection.background = v,
            theme,
            cx,
        ));
        rows.push(self.toggle_row(
            "selection-rounded",
            "Rounded corners",
            "Round the outer corners of terminal text selections.",
            |s| s.terminal.selection.rounded,
            |s, v| s.terminal.selection.rounded = v,
            theme,
            cx,
        ));
        rows.push(self.toggle_row(
            "copy-animation",
            "Play animation on copy",
            "Shine across the selection to confirm each copy.",
            |s| s.terminal.selection.copy_animation,
            |s, v| s.terminal.selection.copy_animation = v,
            theme,
            cx,
        ));
        rows.push(section_label("Scrolling", theme));
        rows.push(self.toggle_row(
            "smooth-scrolling",
            "Smooth scrolling",
            "Move through terminal history by pixels instead of whole lines, on a touchpad.",
            |s| s.terminal.smooth_scrolling,
            |s, v| s.terminal.smooth_scrolling = v,
            theme,
            cx,
        ));
        rows.push(section_label("Resizing", theme));
        rows.push(self.toggle_row(
            "smooth-resizing",
            "Smooth terminal resizing",
            "Reflow terminal text on the frame a pane changes size, without waiting for the server.",
            |s| s.terminal.smooth_resizing,
            |s, v| s.terminal.smooth_resizing = v,
            theme,
            cx,
        ));
        rows.push(section_label("Keyboard", theme));
        rows.push(self.toggle_row(
            "key-hints",
            "Show key sequence hints",
            "Show the keys that can come next after the first key of a sequence, such as a tmux-style prefix.",
            |s| s.terminal.key_hints,
            |s, v| s.terminal.key_hints = v,
            theme,
            cx,
        ));
        rows.push(section_label("Integrated path browser", theme));
        rows.push(self.toggle_row(
            "recent-paths",
            "Show recently used paths",
            "List the paths you chose most recently first, shared by every session on a host.",
            |s| s.terminal.recent_paths,
            |s, v| s.terminal.recent_paths = v,
            theme,
            cx,
        ));
        rows.push(section_label("Links", theme));
        rows.push(self.choice_row(
            "app-links",
            "Links that open apps",
            "Whether ctrl+click opens links such as ssh:// that launch another app. Web, mail and file links always open.",
            &[
                (AppLinks::Ask, "Ask"),
                (AppLinks::Always, "Always"),
                (AppLinks::Never, "Never"),
            ],
            t.app_links,
            |s, v| s.terminal.app_links = v,
            theme,
            cx,
        ));
        let trusted = &t.trusted_links;
        rows.push(
            row(
                "trusted-links",
                "Trusted links",
                if trusted.is_empty() {
                    "None yet. Always Open in the link prompt adds one."
                } else {
                    "Links chosen to always open, by app and host."
                },
                theme,
            )
            .into_any_element(),
        );
        for (index, key) in trusted.iter().enumerate() {
            let remove = key.clone();
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .pb(px(6.))
                    .child(div().flex_1().text_size(px(13.)).child(key.clone()))
                    .child(
                        button(format!("trusted-remove-{index}"), "Remove", false, theme).on_click(
                            cx.listener(move |ws, _, _, cx| {
                                let remove = remove.clone();
                                ws.update_settings(
                                    move |s| s.terminal.trusted_links.retain(|k| *k != remove),
                                    cx,
                                )
                            }),
                        ),
                    )
                    .into_any_element(),
            );
        }
        rows.push(section_label("Titles", theme));
        rows.push(self.toggle_row(
            "prefer-generated-titles",
            "Prefer generated titles",
            "Show the running program and its directory instead of titles that shells set.",
            |s| s.terminal.prefer_generated_titles,
            |s, v| s.terminal.prefer_generated_titles = v,
            theme,
            cx,
        ));
        rows.push(section_label("Shell", theme));
        rows.push(Self::field_row(
            "startup-command",
            "Startup command",
            "The program and arguments new terminals on this computer run, or empty for the login shell. Enter applies.",
            &page.startup,
            theme,
        ));
        rows.push(section_label("Advanced font", theme));
        rows.push(self.stepper_row(
            "character-spacing",
            "Character spacing",
            "Widen or tighten each character's cell.",
            format!("{:+.0} px", t.character_spacing),
            |s| s.terminal.character_spacing = (s.terminal.character_spacing - 1.).max(-4.),
            |s| s.terminal.character_spacing = (s.terminal.character_spacing + 1.).min(16.),
            theme,
            cx,
        ));
        rows.push(self.toggle_row(
            "ligatures",
            "Ligatures",
            "Join characters like -> and != into single symbols.",
            |s| s.terminal.ligatures,
            |s, v| s.terminal.ligatures = v,
            theme,
            cx,
        ));
        rows.push(self.choice_row(
            "text-styles",
            "Render bold and italic",
            "Which styled text draws in its style; the rest draws plain.",
            &[
                (TextStyles::Both, "Both"),
                (TextStyles::BoldOnly, "Bold"),
                (TextStyles::ItalicOnly, "Italic"),
                (TextStyles::Neither, "Neither"),
            ],
            t.text_styles,
            |s, v| s.terminal.text_styles = v,
            theme,
            cx,
        ));
        rows.push(Self::field_row(
            "fallback-fonts",
            "Fallback fonts",
            "For characters the terminal font lacks, in order, separated by commas. Enter applies.",
            &page.fallbacks,
            theme,
        ));
        rows.push(section_label("Colors", theme));
        rows.push(self.toggle_row(
            "improve-contrast",
            "Improve text contrast",
            "Lift text that's too faint to read against its background.",
            |s| s.terminal.improve_contrast,
            |s, v| s.terminal.improve_contrast = v,
            theme,
            cx,
        ));
        rows.push(self.toggle_row(
            "theme-palette",
            "Force theme palette for text",
            "Draw program colors as the nearest theme color, readable on its background.",
            |s| s.terminal.theme_palette,
            |s, v| s.terminal.theme_palette = v,
            theme,
            cx,
        ));
        rows
    }

    /// The theme's selection color, the accent, (for text) the program's own colors, or one
    /// of the 16 terminal colors, as swatches.
    #[allow(clippy::too_many_arguments)]
    fn selection_color_row(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        current: SelectionColor,
        keep: bool,
        set: fn(&mut Settings, SelectionColor),
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut named = vec![
            (SelectionColor::Theme, "Theme"),
            (SelectionColor::Accent, "Accent"),
        ];
        if keep {
            named.push((SelectionColor::Keep, "Keep program colors"));
        }
        let buttons = named.into_iter().map(|(value, label)| {
            button(format!("{id}-{label}"), label, value == current, theme).on_click(
                cx.listener(move |ws, _, _, cx| ws.update_settings(|s| set(s, value), cx)),
            )
        });
        let swatches = (0..16u8).map(|index| {
            let chosen = current == SelectionColor::Palette(index);
            div()
                .id(SharedString::from(format!("{id}-swatch-{index}")))
                .size(px(14.))
                .rounded(theme.radius_small())
                .bg(crate::ui::theme::color(
                    theme.terminal.ansi[usize::from(index)],
                ))
                .cursor_pointer()
                .when(chosen, |d| d.border_2().border_color(theme.text))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.update_settings(|s| set(s, SelectionColor::Palette(index)), cx)
                }))
        });
        row(id, title, detail, theme)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .items_end()
                    .gap(px(6.))
                    .child(div().flex().gap(px(4.)).children(buttons))
                    .child(div().grid().grid_cols(8).gap(px(3.)).children(swatches)),
            )
            .into_any_element()
    }

    /// Accent, the terminal text color, or one of the 16 terminal colors, as swatches.
    fn highlight_color_row(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let current = self.settings.interface.highlight_color;
        let named = [
            (HighlightColor::Accent, "Accent"),
            (HighlightColor::Text, "Text"),
        ];
        let buttons = named.into_iter().map(|(value, label)| {
            button(format!("highlight-{label}"), label, value == current, theme).on_click(
                cx.listener(move |ws, _, _, cx| {
                    ws.update_settings(|s| s.interface.highlight_color = value, cx)
                }),
            )
        });
        let swatches = (0..16u8).map(|index| {
            let chosen = current == HighlightColor::Palette(index);
            div()
                .id(SharedString::from(format!("highlight-swatch-{index}")))
                .size(px(14.))
                .rounded(theme.radius_small())
                .bg(crate::ui::theme::color(
                    theme.terminal.ansi[usize::from(index)],
                ))
                .cursor_pointer()
                .when(chosen, |d| d.border_2().border_color(theme.text))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.update_settings(
                        |s| s.interface.highlight_color = HighlightColor::Palette(index),
                        cx,
                    )
                }))
        });
        row(
            "highlight-color",
            "Highlight color",
            "The accent, the terminal's text color, or one of its 16 colors.",
            theme,
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .items_end()
                .gap(px(6.))
                .child(div().flex().gap(px(4.)).children(buttons))
                .child(div().grid().grid_cols(8).gap(px(3.)).children(swatches)),
        )
        .into_any_element()
    }

    /// A small picture of the window: click where notices should float.
    fn notice_position_row(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let current = self.settings.notifications.notice_position;
        let cell = |position: NoticePosition| {
            let chosen = position == current;
            div()
                .id(SharedString::from(format!("notice-{}", position.label())))
                .flex()
                .flex_1()
                .p(px(3.))
                .cursor_pointer()
                .when(position.top(), |d| d.items_start())
                .when(!position.top(), |d| d.items_end())
                .map(|d| match position.column() {
                    0 => d.justify_start(),
                    2 => d.justify_end(),
                    _ => d.justify_center(),
                })
                .child(
                    div()
                        .w(px(26.))
                        .h(px(8.))
                        .rounded(theme.radius_small())
                        .when(chosen, |d| d.bg(theme.primary))
                        .when(!chosen, |d| {
                            d.bg(theme.overlay(0.1)).hover(|d| d.bg(theme.overlay(0.2)))
                        }),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.update_settings(|s| s.notifications.notice_position = position, cx)
                }))
        };
        let half = |positions: &[NoticePosition]| {
            div()
                .flex()
                .flex_1()
                .children(positions.iter().map(|p| cell(*p)))
        };
        row(
            "notice-position",
            "Notification position",
            current.label(),
            theme,
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .w(px(132.))
                .h(px(80.))
                .rounded(theme.radius_small())
                .bg(theme.background)
                .when(theme.card_borders, |d| {
                    d.border(theme.border_width).border_color(theme.border)
                })
                .child(half(&NoticePosition::ALL[..3]))
                .child(half(&NoticePosition::ALL[3..])),
        )
        .into_any_element()
    }

    fn notifications_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let n = &self.settings.notifications;
        let mut rows = vec![
            self.notice_position_row(theme, cx),
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
        rows.extend(self.ssh_rows(page, theme, cx));
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
            Section::Server => self.server_section(theme, cx),
            Section::Integrations => self.integrations_section(theme, cx),
            Section::Developer => self.developer_section(page, theme, cx),
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
                    // Rows keep their height and the page scrolls, rather than squeezing
                    // them into the panel.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::BOLD)
                                    .pb(px(6.))
                                    .child(page.section.title()),
                            )
                            .children(rows),
                    ),
            )
            .into_any_element()
    }
}
