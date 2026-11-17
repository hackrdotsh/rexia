//! Navigate the focused pane's visible links, or preview one chosen link. Remote
//! files are downloaded before being handed to the desktop previewer or opener.
use super::Workspace;
use crate::{
    quick_look::{self, Target},
    rex::{download, host::PaneCommand},
};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct Preview {
    links: Vec<quick_look::VisibleLink>,
    selected: usize,
    block: String,
    host: usize,
}

/// How a Quick Look was shown, for tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// In GNOME's previewer.
    Previewer(String),
    /// With the desktop's opener: web links, or files without the previewer.
    Opener(String),
}

impl Workspace {
    /// Use the bus at `address` for the previewer instead of the session bus, for tests.
    pub fn set_preview_bus(&mut self, address: Option<String>) {
        self.preview_bus = address;
    }

    /// What Quick Look has shown, newest last, for tests.
    pub fn quick_looks(&self) -> &[Shown] {
        &self.quick_looks
    }

    /// Keep test downloads outside the user's Downloads directory.
    pub fn set_download_directory(&mut self, directory: std::path::PathBuf) {
        self.download_directory = Some(directory);
    }

    fn cancel_preview_work(&mut self) {
        self.preview_generation += 1;
        if let Some(cancel) = self.preview_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Resolve or download a file before handing it to the desktop. Network and disk
    /// work live on their own thread, never in GPUI's event loop.
    pub(super) fn show_quick_target(
        &mut self,
        target: Target,
        preview: bool,
        cx: &mut Context<Self>,
    ) {
        self.cancel_preview_work();
        let generation = self.preview_generation;
        if let Target::Web(link) = target {
            self.open_link(link.clone(), cx);
            self.quick_looks.push(Shown::Opener(link));
            return;
        }
        let Target::Path(path) = target else {
            unreachable!()
        };
        let host = self.host().handle.clone();
        let session = self.model().selected.unwrap_or_default();
        let block = self.focused.clone().unwrap_or_default();
        let cwd = self
            .model()
            .blocks
            .get(&block)
            .and_then(|b| b.pwd.clone().or(b.process.cwd.clone()));
        let directory = self.download_directory.clone();
        let label = self.host().config.label.clone();
        let bus = self.preview_bus.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.preview_cancel = Some(cancelled.clone());
        let (send, receive) = flume::bounded(1);
        std::thread::spawn(move || {
            let result: Result<Shown, String> = (|| {
                let full = if host.endpoint.is_local() {
                    quick_look::resolve(&path, cwd.as_deref())
                        .ok_or_else(|| format!("There's no {path} here"))?
                } else {
                    let directory = directory
                        .map(Ok)
                        .unwrap_or_else(|| download::directory(&label))
                        .map_err(|e| e.to_string())?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    runtime
                        .block_on(download::download(
                            &host, &session, &block, &path, &directory,
                        ))
                        .map_err(|e| format!("Couldn't download {path}: {e:#}"))?
                };
                let uri = quick_look::file_uri(&full);
                if cancelled.load(Ordering::SeqCst) {
                    return Err("Preview cancelled".into());
                }
                if preview && quick_look::preview(bus.as_deref(), &uri).is_ok() {
                    Ok(Shown::Previewer(uri))
                } else {
                    Ok(Shown::Opener(uri))
                }
            })();
            let _ = send.send(result);
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = receive.recv_async().await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                if ws.preview_generation != generation {
                    return;
                }
                match result {
                    Ok(shown) => {
                        if let Shown::Opener(link) = &shown {
                            (ws.link_opener)(link, cx);
                        }
                        ws.quick_looks.push(shown);
                    }
                    Err(error) => ws.notice = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Preview every visible link, starting at the pointer or the first link.
    pub fn quick_look(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.start_quick_look(false, cx)
    }

    /// Context-menu and modified-click previews stay on the chosen link.
    pub fn quick_look_one(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.start_quick_look(true, cx)
    }

    fn start_quick_look(&mut self, single: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let block = self.focused.clone().ok_or("No pane is focused")?;
        let pane = self.panes.get(&block).ok_or("No pane is focused")?.read(cx);
        let pointer = pane.pointer_cell();
        let handle = pane.handle().ok_or("The pane isn't streaming yet")?;
        let frame = handle
            .snapshot()
            .frame
            .ok_or("The pane isn't streaming yet")?;
        let cells: Vec<_> = frame
            .lines
            .iter()
            .enumerate()
            .flat_map(|(y, line)| {
                line.iter()
                    .enumerate()
                    .filter(|(_, cell)| cell.has(crate::rex::stream::flags::LINK))
                    .map(move |(x, _)| (x as u16, y as u16))
            })
            .collect();
        let host = self.active;
        let local = self.host().handle.endpoint.is_local();
        let cwd = self
            .model()
            .blocks
            .get(&block)
            .and_then(|b| b.pwd.clone().or(b.process.cwd.clone()));
        self.cancel_preview_work();
        let generation = self.preview_generation;
        cx.spawn(async move |this, cx| {
            let positions = cells.clone();
            let uris = handle
                .ask(|reply| PaneCommand::LinksAt { cells, reply })
                .await
                .unwrap_or_default();
            let program: Vec<_> = positions
                .into_iter()
                .zip(uris)
                .filter_map(|((x, y), uri)| uri.map(|uri| (x, y, uri)))
                .collect();
            let mut links = quick_look::visible_links(&frame, &program, cwd.as_deref(), local);
            let selected = pointer.and_then(|(x, y)| {
                links
                    .iter()
                    .position(|link| link.row == y && link.columns.contains(&x))
            });
            if single {
                links = selected
                    .map(|index| vec![links.remove(index)])
                    .unwrap_or_default();
            }
            let _ = this.update(cx, |ws, cx| {
                if ws.preview_generation != generation
                    || ws.active != host
                    || ws.focused.as_ref() != Some(&block)
                {
                    return;
                }
                if links.is_empty() {
                    ws.notice = Some(
                        if single {
                            "There's no file or link under the pointer"
                        } else {
                            "No file or web links are visible in this pane"
                        }
                        .into(),
                    );
                } else {
                    let selected = if single { 0 } else { selected.unwrap_or(0) };
                    let target = links[selected].target.clone();
                    ws.quick_preview = Some(Preview {
                        links,
                        selected,
                        block,
                        host,
                    });
                    ws.show_quick_target(target, true, cx);
                }
                cx.notify();
            });
        })
        .detach();
        Ok(())
    }

    pub fn quick_look_selection(&self) -> Option<(usize, usize, Target)> {
        let preview = self.quick_preview.as_ref()?;
        Some((
            preview.selected,
            preview.links.len(),
            preview.links[preview.selected].target.clone(),
        ))
    }

    pub fn close_quick_look(&mut self, cx: &mut Context<Self>) {
        self.quick_preview = None;
        self.cancel_preview_work();
        cx.notify();
    }

    pub fn step_quick_look(&mut self, direction: i32, cx: &mut Context<Self>) {
        let Some(preview) = &mut self.quick_preview else {
            return;
        };
        if preview.host != self.active || self.focused.as_ref() != Some(&preview.block) {
            self.close_quick_look(cx);
            return;
        }
        preview.selected =
            (preview.selected as i32 + direction).rem_euclid(preview.links.len() as i32) as usize;
        let target = preview.links[preview.selected].target.clone();
        self.show_quick_target(target, true, cx);
        cx.notify();
    }

    pub(super) fn quick_look_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.overlay.is_some() || self.context_menu.is_some() {
            return false;
        }
        if self.quick_preview.is_none() {
            return false;
        }
        if self.quick_preview.as_ref().is_some_and(|preview| {
            preview.host != self.active || self.focused.as_ref() != Some(&preview.block)
        }) {
            self.close_quick_look(cx);
            return false;
        }
        match key {
            "left" | "up" => self.step_quick_look(-1, cx),
            "right" | "down" => self.step_quick_look(1, cx),
            "escape" | "space" => self.close_quick_look(cx),
            _ => {
                self.close_quick_look(cx);
                return false;
            }
        }
        true
    }

    pub(super) fn render_quick_look(
        &self,
        theme: &crate::ui::theme::Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self.quick_preview.as_ref()?;
        if preview.host != self.active || self.focused.as_ref() != Some(&preview.block) {
            return None;
        }
        let (Target::Path(title) | Target::Web(title)) = &preview.links[preview.selected].target;
        let button = |id, icon, action: fn(&mut Self, &mut Context<Self>)| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(6.))
                .cursor_pointer()
                .child(icon)
                .on_click(cx.listener(move |ws, _, _, cx| action(ws, cx)))
        };
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .left(px(24.))
                .right(px(24.))
                .occlude()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(8.))
                .bg(theme.surface_high)
                .text_color(theme.text)
                .rounded(theme.radius())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(title.clone()),
                )
                .child(format!(
                    "{} / {}",
                    preview.selected + 1,
                    preview.links.len()
                ))
                .child(button("preview-previous", "←", |ws, cx| {
                    ws.step_quick_look(-1, cx)
                }))
                .child(button("preview-next", "→", |ws, cx| {
                    ws.step_quick_look(1, cx)
                }))
                .child(button("preview-close", "×", Self::close_quick_look))
                .into_any_element(),
        )
    }
}
