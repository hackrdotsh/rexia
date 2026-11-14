//! `client.quick_look`: preview the file or web link under the pointer in the focused
//! pane: a program's link (OSC 8) first, else a path or URL in the text there.
use super::Workspace;
use crate::{
    quick_look::{self, Target},
    rex::host::PaneCommand,
};
use gpui::Context;

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

    /// Preview what's under the pointer in the focused pane.
    pub fn quick_look(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let block = self.focused.clone().ok_or("No pane is focused")?;
        let pane = self.panes.get(&block).ok_or("No pane is focused")?.read(cx);
        let (x, y) = pane
            .pointer_cell()
            .ok_or("Point at a file or link in the pane to preview it")?;
        let handle = pane.handle().ok_or("The pane isn't streaming yet")?;
        let line = handle
            .snapshot()
            .frame
            .map(|f| f.line_text(usize::from(y)))
            .unwrap_or_default();
        let model = self.model();
        let cwd = model
            .blocks
            .get(&block)
            .and_then(|b| b.pwd.clone().or(b.process.cwd.clone()));
        let local = self.host().handle.endpoint.is_local();
        let bus = self.preview_bus.clone();
        cx.spawn(async move |this, cx| {
            // A program's own link wins over the text it's shown on.
            let link = handle
                .ask(|reply| PaneCommand::LinkAt { x, y, reply })
                .await
                .flatten();
            let target = match link {
                Some(link) if link.starts_with("file://") => Some(Target::Path(
                    url::Url::parse(&link)
                        .ok()
                        .and_then(|u| u.to_file_path().ok())
                        .map_or(link.clone(), |p| p.display().to_string()),
                )),
                Some(link) => Some(Target::Web(link)),
                None => quick_look::target_at(&line, usize::from(x)),
            };
            let outcome: Result<Shown, String> = match target {
                None => Err("There's no file or link under the pointer".into()),
                Some(Target::Web(url)) => Ok(Shown::Opener(url)),
                Some(Target::Path(_)) if !local => {
                    Err("That file is on the remote host, so it can't be previewed here".into())
                }
                Some(Target::Path(path)) => match quick_look::resolve(&path, cwd.as_deref()) {
                    None => Err(format!("There's no {path} here")),
                    Some(full) => {
                        let uri = quick_look::file_uri(&full);
                        let shown = uri.clone();
                        let previewed = cx
                            .background_executor()
                            .spawn(async move { quick_look::preview(bus.as_deref(), &shown) })
                            .await;
                        Ok(match previewed {
                            Ok(()) => Shown::Previewer(uri),
                            // Without GNOME's previewer, the desktop's opener shows it.
                            Err(_) => Shown::Opener(uri),
                        })
                    }
                },
            };
            let _ = this.update(cx, |ws, cx| {
                match outcome {
                    Ok(Shown::Opener(uri)) => {
                        (ws.link_opener.clone())(&uri, cx);
                        ws.quick_looks.push(Shown::Opener(uri));
                    }
                    Ok(shown) => ws.quick_looks.push(shown),
                    Err(why) => ws.notice = Some(why),
                }
                cx.notify();
            });
        })
        .detach();
        Ok(())
    }
}
