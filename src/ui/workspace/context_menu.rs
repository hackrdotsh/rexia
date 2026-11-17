//! The terminal's context menu: right-click a pane (Shift+right-click when its program uses
//! the mouse). Editing first, then what's under the pointer (a link or a file), then the
//! pane's own actions, as Rex for Mac orders them.
use super::{Workspace, actions::Target};
use crate::{
    quick_look::{self, Target as Under},
    rex::host::PaneCommand,
    ui::theme::Theme,
};
use gpui::{
    AnyElement, ClipboardItem, Context, IntoElement, ParentElement, Pixels, Point, SharedString,
    Styled, Window, anchored, deferred, div, prelude::*, px,
};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// One of rexia's actions, or a block action the server knows (Clear, Reset).
    Run(&'static str),
    OpenLink(String),
    QuickLook,
    DownloadFile(String),
    Copy(String),
    ShowInFiles(PathBuf),
    /// Something done to a tab, by its window ID.
    Tab(TabCommand, String),
    /// Something done to a session: on this host, by its ID.
    Session(super::sessions::SessionCommand, usize, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabCommand {
    Rename,
    Mute,
    CloseOthers,
    Close,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub label: String,
    pub command: Command,
    pub enabled: bool,
    /// The action's shortcut, when it has one.
    pub(super) shortcut: Option<String>,
}

pub struct ContextMenu {
    session: String,
    block: String,
    at: Point<Pixels>,
    /// Groups, drawn with a line between them.
    groups: Vec<Vec<Item>>,
    highlighted: Option<usize>,
}

impl ContextMenu {
    /// A menu about `session` (not a pane), shown at `at`.
    pub(super) fn new(session: String, at: Point<Pixels>, groups: Vec<Vec<Item>>) -> Self {
        Self {
            session,
            block: String::new(),
            at,
            groups,
            highlighted: None,
        }
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.groups.iter().flatten()
    }
}

impl Workspace {
    /// Right-click on `block`'s cell `cell`: find what's there, then show the menu.
    pub(super) fn open_context_menu(
        &mut self,
        block: String,
        at: Point<Pixels>,
        cell: (u16, u16),
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.panes.get(&block).and_then(|p| p.read(cx).handle()) else {
            return;
        };
        let session = self.model().selected.clone().unwrap_or_default();
        let snapshot = pane.snapshot();
        let selected = snapshot.frame.as_ref().is_some_and(|f| f.has_selection);
        let line = snapshot
            .frame
            .as_ref()
            .map(|f| f.line_text(usize::from(cell.1)))
            .unwrap_or_default();
        let cwd = self
            .model()
            .blocks
            .get(&block)
            .and_then(|b| b.pwd.clone().or(b.process.cwd.clone()));
        let local = self.host().handle.endpoint.is_local();
        cx.spawn(async move |this, cx| {
            let (x, y) = cell;
            let link = pane
                .ask(|reply| PaneCommand::LinkAt { x, y, reply })
                .await
                .flatten();
            let under = match link {
                Some(link) if link.starts_with("file://") => url::Url::parse(&link)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| Under::Path(p.display().to_string())),
                Some(link) => Some(Under::Web(link)),
                None => quick_look::target_at(&line, usize::from(x)),
            };
            let _ = this.update(cx, |ws, cx| {
                let groups = ws.context_items(selected, under, cwd.as_deref(), local);
                ws.context_menu = Some(Box::new(ContextMenu {
                    session,
                    block,
                    at,
                    groups,
                    highlighted: None,
                }));
                cx.notify();
            });
        })
        .detach();
    }

    fn context_items(
        &self,
        selected: bool,
        under: Option<Under>,
        cwd: Option<&str>,
        local: bool,
    ) -> Vec<Vec<Item>> {
        let item = |label: &str, command: Command, enabled: bool| {
            let shortcut = match &command {
                Command::Run(action) => {
                    Some(self.shortcut_of(action)).filter(|s| !s.is_empty() && s != "None")
                }
                _ => None,
            };
            Item {
                label: label.to_owned(),
                command,
                enabled,
                shortcut,
            }
        };
        let mut groups = vec![vec![
            item("Copy", Command::Run("pane.copy"), selected),
            item("Paste", Command::Run("pane.paste"), true),
            item("Select All", Command::Run("pane.select_all"), true),
        ]];
        let mut here = Vec::new();
        match under {
            Some(Under::Web(link)) => {
                here.push(item("Open Link", Command::OpenLink(link.clone()), true));
                here.push(item("Copy Link", Command::Copy(link), true));
                here.push(item("Quick Look", Command::QuickLook, true));
            }
            Some(Under::Path(path)) => {
                let found = local.then(|| quick_look::resolve(&path, cwd)).flatten();
                if let Some(full) = found {
                    here.push(item(
                        "Open File",
                        Command::OpenLink(quick_look::file_uri(&full)),
                        true,
                    ));
                    here.push(item(
                        "Copy Path",
                        Command::Copy(full.display().to_string()),
                        true,
                    ));
                    here.push(item("Show in Files", Command::ShowInFiles(full), true));
                    here.push(item("Quick Look", Command::QuickLook, true));
                } else if !local {
                    here.push(item("Open File", Command::DownloadFile(path.clone()), true));
                    here.push(item("Copy Path", Command::Copy(path), true));
                    here.push(item("Quick Look", Command::QuickLook, true));
                } else {
                    here.push(item("Copy Path", Command::Copy(path), true));
                }
            }
            None => {}
        }
        if !here.is_empty() {
            groups.push(here);
        }
        groups.push(vec![
            item("Split Right", Command::Run("pane.split.right"), true),
            item("Split Down", Command::Run("pane.split.down"), true),
            item("Zoom Pane", Command::Run("pane.zoom"), true),
            item("Float Pane", Command::Run("pane.float"), true),
        ]);
        groups.push(vec![
            item(
                "Clear Terminal",
                Command::Run("com.superlogical.terminal.clear"),
                true,
            ),
            item(
                "Reset Terminal",
                Command::Run("com.superlogical.terminal.reset"),
                true,
            ),
            item("Copy Pane ID", Command::Run("pane.copy_id"), true),
        ]);
        groups.push(vec![item("Close Pane", Command::Run("pane.close"), true)]);
        groups
    }

    /// Right-click on a tab: rename, mute and close it, as Rex's tab menu does.
    pub fn open_tab_menu(&mut self, window_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        let model = self.model();
        let Some(session) = model.selected.clone() else {
            return;
        };
        let tabs = model.view().map_or(0, |v| v.windows.len());
        let muted = self.tab_muted(self.active, &window_id);
        let item = |label: &str, command: TabCommand, action: &str, enabled: bool| Item {
            label: label.to_owned(),
            command: Command::Tab(command, window_id.clone()),
            enabled,
            shortcut: Some(self.shortcut_of(action)).filter(|s| !s.is_empty() && s != "None"),
        };
        let groups = vec![
            vec![
                item("Rename Tab…", TabCommand::Rename, "client.tab.rename", true),
                item(
                    if muted { "Unmute Tab" } else { "Mute Tab" },
                    TabCommand::Mute,
                    "client.tab.mute.toggle",
                    true,
                ),
            ],
            vec![
                item(
                    "Close Other Tabs",
                    TabCommand::CloseOthers,
                    "client.tab.close_others",
                    tabs > 1,
                ),
                item("Close Tab", TabCommand::Close, "client.tab.close", true),
            ],
        ];
        self.context_menu = Some(Box::new(ContextMenu {
            session,
            block: String::new(),
            at,
            groups,
            highlighted: None,
        }));
        cx.notify();
    }

    /// Whether a right-click menu (a pane's or a tab's) is open.
    pub fn context_menu_open(&self) -> bool {
        self.context_menu.is_some()
    }

    /// The menu's labels, in order, while it's open, for tests. Disabled ones end in `(off)`.
    pub fn context_menu_labels(&self) -> Option<Vec<String>> {
        let menu = self.context_menu.as_ref()?;
        Some(
            menu.items()
                .map(|i| {
                    if i.enabled {
                        i.label.clone()
                    } else {
                        format!("{} (off)", i.label)
                    }
                })
                .collect(),
        )
    }

    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Choose the item labeled `label`.
    pub fn choose_context_item(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        let Some(item) = menu
            .items()
            .find(|i| i.label == label && i.enabled)
            .cloned()
        else {
            return;
        };
        let target = Target {
            session_id: Some(menu.session.clone()),
            block_id: Some(menu.block.clone()),
        };
        match item.command {
            Command::Run(action) => {
                if let Err((_, why)) =
                    self.run_action(action, Value::Null, "context_menu", target, window, cx)
                {
                    self.notice = Some(why);
                }
            }
            Command::OpenLink(link) => self.open_link(link, cx),
            Command::QuickLook => {
                if let Err(error) = self.quick_look_one(cx) {
                    self.notice = Some(error);
                }
            }
            Command::DownloadFile(path) => self.show_quick_target(Under::Path(path), false, cx),
            Command::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Command::ShowInFiles(path) => self.show_in_files(path, cx),
            Command::Tab(command, id) => self.tab_command(command, id, window, cx),
            Command::Session(command, host, id) => {
                self.session_command(command, host, id, window, cx)
            }
        }
        cx.notify();
    }

    pub(super) fn tab_command(
        &mut self,
        command: TabCommand,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            TabCommand::Rename => {
                let model = self.model();
                let label = model
                    .view()
                    .and_then(|v| v.window(&id))
                    .map(|w| crate::ui::titles::tab_title(w, &model))
                    .unwrap_or_default();
                self.open_rename(super::overlay::RenameTarget::Window(id), label, window, cx);
            }
            TabCommand::Mute => self.toggle_tab_mute(self.active, id, cx),
            TabCommand::CloseOthers => {
                self.request_close(super::close_guard::CloseTarget::OtherTabs(id), cx)
            }
            TabCommand::Close => self.request_close(super::close_guard::CloseTarget::Tab(id), cx),
        }
    }

    /// Show a file in the file manager, or open its folder when there's none to ask.
    pub fn show_in_files(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let bus = self.preview_bus.clone();
        let uri = quick_look::file_uri(&path);
        let folder = path
            .parent()
            .map(quick_look::file_uri)
            .unwrap_or_else(|| uri.clone());
        cx.spawn(async move |this, cx| {
            let shown = cx
                .background_executor()
                .spawn(async move { quick_look::show_in_files(bus.as_deref(), &uri) })
                .await;
            if shown.is_err() {
                let _ = this.update(cx, |ws, cx| (ws.link_opener.clone())(&folder, cx));
            }
        })
        .detach();
    }

    /// Keys while the menu is open: arrows move, Enter chooses, Escape closes. Other keys
    /// close it and go on to the terminal.
    pub(super) fn context_menu_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = &mut self.context_menu else {
            return false;
        };
        let enabled: Vec<usize> = menu
            .items()
            .enumerate()
            .filter(|(_, i)| i.enabled)
            .map(|(n, _)| n)
            .collect();
        let step = |by: isize, now: Option<usize>| {
            let at = now.and_then(|n| enabled.iter().position(|e| *e == n));
            let len = enabled.len() as isize;
            let next = match at {
                Some(at) => (at as isize + by).rem_euclid(len),
                None if by > 0 => 0,
                None => len - 1,
            };
            enabled.get(next as usize).copied()
        };
        match key {
            "down" => menu.highlighted = step(1, menu.highlighted),
            "up" => menu.highlighted = step(-1, menu.highlighted),
            "enter" => {
                let label = menu
                    .highlighted
                    .and_then(|n| menu.items().nth(n))
                    .map(|i| i.label.clone());
                match label {
                    Some(label) => self.choose_context_item(&label, window, cx),
                    None => self.close_context_menu(cx),
                }
            }
            "escape" => self.close_context_menu(cx),
            _ => {
                self.close_context_menu(cx);
                return false;
            }
        }
        cx.notify();
        true
    }

    pub(super) fn render_context_menu(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let mut list = div()
            .id("context-menu")
            .occlude()
            .min_w(px(220.))
            .flex()
            .flex_col()
            .py(px(4.))
            .bg(theme.surface)
            .rounded(theme.radius_small())
            .when(theme.popup_borders, |d| {
                d.border(theme.border_width).border_color(theme.border)
            })
            .when(theme.popup_shadows, |d| d.shadow_lg())
            .font_family(theme.ui_font.clone())
            .text_size(px(13.))
            .text_color(theme.text)
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| ws.close_context_menu(cx)));
        let mut index = 0;
        for (n, group) in menu.groups.iter().enumerate() {
            if n > 0 {
                list = list.child(div().my(px(4.)).h(px(1.)).bg(theme.overlay(0.12)));
            }
            for item in group {
                let highlighted = menu.highlighted == Some(index);
                let label = item.label.clone();
                let row = div()
                    .id(SharedString::from(format!("context-{}", item.label)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(24.))
                    .mx(px(4.))
                    .px(px(10.))
                    .h(px(26.))
                    .rounded(theme.radius_small())
                    .when(highlighted, |d| {
                        d.bg(theme.primary).text_color(theme.on_primary)
                    })
                    .when(!item.enabled, |d| {
                        d.text_color(theme.text_muted.opacity(0.6))
                    })
                    .when(item.enabled, |d| {
                        d.cursor_pointer()
                            .when(!highlighted, |d| d.hover(|d| d.bg(theme.overlay(0.08))))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.choose_context_item(&label, window, cx)
                            }))
                    })
                    .child(item.label.clone())
                    .children(item.shortcut.clone().map(|s| {
                        div()
                            .text_size(px(12.))
                            .text_color(if highlighted {
                                theme.on_primary
                            } else {
                                theme.text_muted
                            })
                            .child(s)
                    }));
                list = list.child(row);
                index += 1;
            }
        }
        Some(
            deferred(
                anchored()
                    .position(menu.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
