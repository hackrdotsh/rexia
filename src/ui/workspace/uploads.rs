//! Files dropped or pasted on a terminal on another machine: they're uploaded there and the
//! terminal types their paths. An upload that takes a moment shows its progress with Cancel;
//! one that fails says why.
use super::Workspace;
use crate::{
    rex::upload::{self, Progress, Source},
    ui::{icons::icon, theme::Theme},
};
use gpui::{
    AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px,
    relative,
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// Uploads show their progress once they've taken this long.
const QUIET: Duration = Duration::from_millis(400);

pub struct Upload {
    id: u64,
    label: String,
    started: Instant,
    progress: Arc<Progress>,
}

impl Workspace {
    /// Upload `files` to the machine `block`'s terminal runs on.
    pub(super) fn start_upload(
        &mut self,
        block: String,
        files: Vec<Source>,
        cx: &mut Context<Self>,
    ) {
        let session = self.model().selected.clone().unwrap_or_default();
        let host = self.host().handle.clone();
        let label = match files.as_slice() {
            [one] => format!("Uploading {} to {}", one.name(), self.host().config.label),
            many => format!(
                "Uploading {} files to {}",
                many.len(),
                self.host().config.label
            ),
        };
        let progress = Arc::new(Progress::default());
        self.next_upload += 1;
        let id = self.next_upload;
        self.uploads.push(Upload {
            id,
            label,
            started: Instant::now(),
            progress: progress.clone(),
        });
        let running = progress.clone();
        // A thread of its own: a big upload over a slow link takes minutes.
        let (done, finished) = flume::bounded(1);
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(anyhow::Error::from)
                .and_then(|runtime| {
                    runtime.block_on(upload::upload(&host, &session, &block, files, running))
                });
            let _ = done.send(result);
        });
        cx.spawn(async move |this, cx| {
            // Redraw the progress while it runs.
            let result = loop {
                let tick = cx.background_executor().timer(Duration::from_millis(100));
                match futures_util::future::select(finished.recv_async(), std::pin::pin!(tick))
                    .await
                {
                    futures_util::future::Either::Left((result, _)) => {
                        break result
                            .unwrap_or_else(|_| Err(anyhow::anyhow!("the upload stopped")));
                    }
                    futures_util::future::Either::Right(_) => {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                }
            };
            let _ = this.update(cx, |ws, cx| {
                ws.uploads.retain(|u| u.id != id);
                ws.last_upload = Some(match result {
                    Ok(()) => Ok(()),
                    Err(_) if progress.cancelled.load(Ordering::SeqCst) => {
                        Err("Upload cancelled".to_owned())
                    }
                    Err(error) => Err(format!("Couldn't upload: {error:#}")),
                });
                if let Some(Err(why)) = &ws.last_upload
                    && why != "Upload cancelled"
                {
                    ws.notice = Some(why.clone());
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// A paste that could run a command before it's read: show it and ask first.
    pub(super) fn confirm_paste(&mut self, block: String, text: String, cx: &mut Context<Self>) {
        let lines = text.lines().count().max(1);
        let mut preview: String = text.lines().take(6).collect::<Vec<_>>().join("\n");
        if preview.chars().count() > 400 {
            preview = preview.chars().take(400).collect::<String>() + "…";
        }
        if lines > 6 {
            preview.push_str(&format!("\n… and {} more lines", lines - 6));
        }
        self.open_confirm(
            "Paste text that runs as it's pasted?".into(),
            format!(
                "The program in this pane hasn't asked for bracketed paste, so each line break \
                 runs what comes before it.\n\n{preview}"
            ),
            "Paste".into(),
            super::overlay::Confirm::Paste { block, text },
            cx,
        );
    }

    /// Uploads under way, as (what, bytes sent, bytes in all), for tests.
    pub fn uploads(&self) -> Vec<(String, u64, u64)> {
        self.uploads
            .iter()
            .map(|u| {
                (
                    u.label.clone(),
                    u.progress.sent.load(Ordering::SeqCst),
                    u.progress.total.load(Ordering::SeqCst),
                )
            })
            .collect()
    }

    /// How the last upload ended, for tests.
    pub fn last_upload(&self) -> Option<&Result<(), String>> {
        self.last_upload.as_ref()
    }

    pub fn cancel_uploads(&mut self) {
        for upload in &self.uploads {
            upload.progress.cancel();
        }
    }

    pub(super) fn render_uploads(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let showing: Vec<&Upload> = self
            .uploads
            .iter()
            .filter(|u| u.started.elapsed() >= QUIET)
            .collect();
        if showing.is_empty() {
            return None;
        }
        let position = self.settings.notifications.notice_position;
        // Beside the notice when there is one, not on top of it.
        let offset = if self.notice.is_some() { 76. } else { 0. };
        let cards = showing.into_iter().map(|u| {
            let sent = u.progress.sent.load(Ordering::SeqCst);
            let total = u.progress.total.load(Ordering::SeqCst).max(1);
            let fraction = (sent as f32 / total as f32).clamp(0., 1.);
            let progress = u.progress.clone();
            div()
                .id(SharedString::from(format!("upload-{}", u.id)))
                .occlude()
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
                .child(icon("loader-2").size(px(16.)).text_color(theme.primary))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_size(px(13.)).child(format!(
                            "{}… {}%",
                            u.label,
                            (fraction * 100.).round()
                        )))
                        .child(
                            div()
                                .h(px(4.))
                                .w_full()
                                .rounded_full()
                                .bg(theme.overlay(0.12))
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(fraction))
                                        .rounded_full()
                                        .bg(theme.primary),
                                ),
                        ),
                )
                .child(
                    super::preferences::button(
                        format!("upload-cancel-{}", u.id),
                        "Cancel",
                        false,
                        theme,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        progress.cancel();
                        cx.notify();
                    })),
                )
        });
        Some(
            div()
                .absolute()
                .when(position.top(), |d| d.top(px(60. + offset)))
                .when(!position.top(), |d| d.bottom(px(16. + offset)))
                .map(|d| match position.column() {
                    0 => d.left(px(16.)),
                    2 => d.right(px(16.)),
                    _ => d.left(relative(0.5)).ml(px(-220.)),
                })
                .w(px(440.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(cards)
                .into_any_element(),
        )
    }
}
