//! Navigate the focused pane's visible links, or preview one chosen link. Remote
//! files are downloaded before being handed to the desktop previewer or opener.
use super::{Workspace, overlay::Confirm};
use crate::{
    quick_look::{self, Target},
    rex::download,
};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use std::path::Path;
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

enum PreparedFile {
    Show(Shown),
    Confirm { uri: String, path: String },
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
        block: &str,
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
        let host_index = self.active;
        let session = self.model().selected.unwrap_or_default();
        let origin_session = session.clone();
        let block = block.to_owned();
        let directory = self.download_directory.clone();
        let label = self.host().config.label.clone();
        let bus = self.preview_bus.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.preview_cancel = Some(cancelled.clone());
        let (send, receive) = flume::bounded(1);
        std::thread::spawn(move || {
            let result: Result<PreparedFile, String> = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                let full = if host.endpoint.is_local() {
                    let cwd = quick_look::needs_cwd(&path)
                        .then(|| runtime.block_on(host.process_directory(&session, &block)))
                        .flatten();
                    quick_look::resolve(&path, cwd.as_deref())
                        .ok_or_else(|| format!("There's no {path} here"))?
                } else {
                    let directory = directory
                        .map(Ok)
                        .unwrap_or_else(|| download::directory(&label))
                        .map_err(|e| e.to_string())?;
                    runtime
                        .block_on(download::download(
                            &host, &session, &block, &path, &directory,
                        ))
                        .map_err(|e| format!("Couldn't download {path}: {e:#}"))?
                };
                if cancelled.load(Ordering::SeqCst) {
                    return Err("Preview cancelled".into());
                }
                let (checked, runnable) = quick_look::inspect_file(&full, Path::new(&path))
                    .map_err(|error| format!("Couldn't open {path}: {error}"))?;
                let uri = quick_look::file_uri(&checked);
                if runnable {
                    return Ok(PreparedFile::Confirm {
                        uri,
                        path: checked.to_string_lossy().escape_debug().to_string(),
                    });
                }
                if preview && quick_look::preview(bus.as_deref(), &uri).is_ok() {
                    Ok(PreparedFile::Show(Shown::Previewer(uri)))
                } else {
                    Ok(PreparedFile::Show(Shown::Opener(uri)))
                }
            })();
            let _ = send.send(result);
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = receive.recv_async().await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                if ws.preview_generation != generation
                    || ws.active != host_index
                    || ws.model().selected.unwrap_or_default() != origin_session
                {
                    return;
                }
                match result {
                    Ok(PreparedFile::Show(shown)) => ws.finish_file_open(shown, cx),
                    Ok(PreparedFile::Confirm { uri, path }) => {
                        ws.open_confirm(
                            "Open this file?".into(),
                            format!("{path}\n\nOpening this file may run code on this computer."),
                            "Open Anyway".into(),
                            Confirm::OpenFile(uri),
                            cx,
                        );
                    }
                    Err(error) => ws.notice = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn finish_file_open(&mut self, shown: Shown, cx: &mut Context<Self>) {
        if let Shown::Opener(link) = &shown {
            (self.link_opener)(link, cx);
        }
        self.quick_looks.push(shown);
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
        let mut links = if single {
            pointer
                .and_then(|(x, y)| quick_look::link_at(&frame, x, y))
                .into_iter()
                .collect()
        } else {
            quick_look::visible_links(&frame)
        };
        let host_index = self.active;
        let host = self.host().handle.clone();
        let session = self.model().selected.unwrap_or_default();
        self.cancel_preview_work();
        let generation = self.preview_generation;
        cx.spawn(async move |this, cx| {
            if host.endpoint.is_local() {
                let needs_cwd = links.iter().any(|link| matches!(&link.target, Target::Path(path) if quick_look::needs_cwd(path)));
                let cwd = if needs_cwd { host.process_directory(&session, &block).await } else { None };
                links = cx.background_executor().spawn(async move {
                    links.retain(|link| match &link.target {
                        Target::Path(path) => quick_look::resolve(path, cwd.as_deref()).is_some(),
                        Target::Web(_) => true,
                    });
                    links
                }).await;
            }
            let selected = pointer.and_then(|(x, y)| {
                links
                    .iter()
                    .position(|link| link.contains(x, y))
            });
            let _ = this.update(cx, |ws, cx| {
                if ws.preview_generation != generation
                    || ws.active != host_index
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
                        block: block.clone(),
                        host: host_index,
                    });
                    ws.show_quick_target(target, true, &block, cx);
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
        let block = preview.block.clone();
        self.show_quick_target(target, true, &block, cx);
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
