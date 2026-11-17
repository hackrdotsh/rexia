//! Terminal › Links: opening the links programs print. Web, mail and file links open on
//! ctrl+click; links that launch other apps, such as ssh://, ask first, always open, or
//! never open, and links you chose to always open are remembered.
use super::{
    Workspace,
    overlay::{Dialog, Overlay},
};
use crate::settings::AppLinks;
use gpui::{App, Context};

/// Opens a link on the desktop, or records it in tests.
pub type LinkOpener = std::rc::Rc<dyn Fn(&str, &mut App)>;

/// Links that open in a browser, mail client or file manager, never another app.
pub fn ordinary(link: &str) -> bool {
    url::Url::parse(link)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto" | "file"))
}

/// What a trusted link is remembered by: its scheme and host, such as `ssh://build`, or
/// its scheme alone for links without a host. `None` for links that never open.
pub fn trust_key(link: &str) -> Option<String> {
    let url = url::Url::parse(link).ok()?;
    // Schemes that run code or carry content in the link itself never open.
    if matches!(
        url.scheme(),
        "javascript" | "data" | "vbscript" | "about" | "blob"
    ) {
        return None;
    }
    Some(match url.host_str() {
        Some(host) => format!("{}://{host}", url.scheme()),
        None => format!("{}:", url.scheme()),
    })
}

/// What clicking `link` does under `policy`, with `trusted` links remembered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkDecision {
    Open,
    Ask,
    Refuse(String),
}

pub fn decide(link: &str, policy: AppLinks, trusted: &[String]) -> LinkDecision {
    if ordinary(link) {
        return LinkDecision::Open;
    }
    let Some(key) = trust_key(link) else {
        return LinkDecision::Refuse("rexia doesn't open links like this one".into());
    };
    match policy {
        AppLinks::Never => {
            LinkDecision::Refuse("Links that open other apps are off in Settings › Terminal".into())
        }
        AppLinks::Always => LinkDecision::Open,
        AppLinks::Ask if trusted.contains(&key) => LinkDecision::Open,
        AppLinks::Ask => LinkDecision::Ask,
    }
}

impl Workspace {
    /// A ctrl+clicked link: open it, ask first, or say why it won't open.
    pub fn open_link(&mut self, link: String, cx: &mut Context<Self>) {
        if url::Url::parse(&link)
            .is_ok_and(|url| url.scheme() == "rex" && url.host_str() == Some("settings"))
        {
            self.pending_settings_link = Some(link);
            cx.notify();
            return;
        }
        if !self.host().handle.endpoint.is_local()
            && let Some(crate::quick_look::Target::Path(path)) = crate::quick_look::from_uri(&link)
        {
            self.show_quick_target(crate::quick_look::Target::Path(path), false, cx);
            return;
        }
        let terminal = &self.settings.terminal;
        match decide(&link, terminal.app_links, &terminal.trusted_links) {
            LinkDecision::Open => self.launch_link(&link, cx),
            LinkDecision::Refuse(why) => {
                self.notice = Some(why);
                cx.notify();
            }
            LinkDecision::Ask => {
                self.overlay = Some(Overlay::Dialog(Dialog::OpenLink { link, focused: 2 }));
                cx.notify();
            }
        }
    }

    /// The link dialog's choice: open once, or (`always`) remember its scheme and host
    /// and open it.
    pub fn answer_link(&mut self, always: bool, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(Overlay::Dialog(Dialog::OpenLink { link, .. })) = &self.overlay else {
            return;
        };
        let link = link.clone();
        self.close_overlay(window, cx);
        if always && let Some(key) = trust_key(&link) {
            self.update_settings(
                move |s| {
                    if !s.terminal.trusted_links.contains(&key) {
                        s.terminal.trusted_links.push(key);
                    }
                },
                cx,
            );
        }
        self.launch_link(&link, cx);
    }

    fn launch_link(&mut self, link: &str, cx: &mut Context<Self>) {
        (self.link_opener)(link, cx);
    }

    /// Open links with `open` instead of the desktop's handler, for tests.
    pub fn set_link_opener(&mut self, open: impl Fn(&str, &mut App) + 'static) {
        self.link_opener = std::rc::Rc::new(open);
    }

    /// The link the dialog asks about, while it's open.
    pub fn asked_link(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::Dialog(Dialog::OpenLink { link, .. })) => Some(link),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_open_ask_or_refuse() {
        let trusted = vec!["ssh://build".to_owned()];
        assert_eq!(
            decide("https://x.dev", AppLinks::Never, &[]),
            LinkDecision::Open
        );
        assert_eq!(
            decide("ssh://me@box", AppLinks::Ask, &trusted),
            LinkDecision::Ask
        );
        assert_eq!(
            decide("ssh://me@build:22", AppLinks::Ask, &trusted),
            LinkDecision::Open
        );
        assert_eq!(
            decide("vscode://file/x", AppLinks::Always, &[]),
            LinkDecision::Open
        );
        assert!(matches!(
            decide("ssh://box", AppLinks::Never, &[]),
            LinkDecision::Refuse(_)
        ));
        assert!(matches!(
            decide("javascript:alert(1)", AppLinks::Always, &[]),
            LinkDecision::Refuse(_)
        ));
        assert_eq!(trust_key("zoommtg:join?x=1").as_deref(), Some("zoommtg:"));
        assert_eq!(
            trust_key("ssh://me@build:22/x").as_deref(),
            Some("ssh://build")
        );
    }
}
