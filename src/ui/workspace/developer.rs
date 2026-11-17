//! Settings › Developer: the local server's API address, and the browser origins allowed
//! to open Rex WebSocket streams. Rex servers take origins as a start option
//! (`--websocket-origin-pattern`), so rexia passes them when it starts the server, with
//! Restart Server; they apply from the next restart.
use super::{
    Workspace,
    preferences::{SettingsPage, button, row},
};
use crate::{rex::endpoint::Endpoint, settings::TerminalAnchor, ui::theme::Theme};
use gpui::{
    AnyElement, ClipboardItem, Context, IntoElement, ParentElement, Styled, div, prelude::*, px,
};

/// Check a browser origin pattern: `https://app.example.com`, `http://localhost:5173`, or
/// a wildcard subdomain such as `https://*.example.com`. The normalized pattern, or why
/// it isn't one.
pub fn origin_pattern(text: &str) -> Result<String, String> {
    let text = text.trim().trim_end_matches('/').to_lowercase();
    let (scheme, rest) = text
        .split_once("://")
        .ok_or("An origin starts with http:// or https://")?;
    if !matches!(scheme, "http" | "https") {
        return Err("An origin starts with http:// or https://".into());
    }
    if rest.is_empty() || rest.contains(['/', '?', '#', ' ']) {
        return Err("An origin is a scheme and host, with no path".into());
    }
    let host = rest.split(':').next().unwrap_or_default();
    let bare = host.strip_prefix("*.").unwrap_or(host);
    if bare.is_empty() || bare.contains('*') {
        return Err("A wildcard can only stand for subdomains, as in https://*.example.com".into());
    }
    Ok(format!("{scheme}://{rest}"))
}

/// What `rex server start` gets, besides the address: the allowed origins.
pub fn server_start_args(address: &str, origins: &[String]) -> Vec<String> {
    let mut args: Vec<String> = ["-S", address, "server", "start"]
        .map(String::from)
        .to_vec();
    for origin in origins {
        args.extend(["--websocket-origin-pattern".into(), origin.clone()]);
    }
    args
}

impl Workspace {
    /// Run `rex` from `location` from now on (an executable, or a source checkout holding
    /// one), or the one on PATH with `None`; then check it runs.
    pub fn choose_rex(&mut self, location: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        // The field shows the location in use.
        if let (Some(location), Some(super::overlay::Overlay::Settings(page))) =
            (&location, &self.overlay)
        {
            let text = location.display().to_string();
            page.rex_source
                .update(cx, |input, cx| input.set_text(text, cx));
        }
        self.update_settings(|s| s.rex_location = location, cx);
        self.check_rex(cx);
    }

    /// Check that the chosen rex runs, in the background.
    pub fn check_rex(&mut self, cx: &mut Context<Self>) {
        let location = self.settings.rex_location.clone();
        self.rex_check = None;
        cx.spawn(async move |this, cx| {
            let checked = location.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::rex::install::check(checked.as_deref()) })
                .await;
            let _ = this.update(cx, |ws, cx| {
                // A check of a location chosen before this one may finish after it; it
                // mustn't replace the current one's result.
                if location == ws.settings.rex_location {
                    ws.rex_check = Some((location, result));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The chosen rex's check, once it's done, for the UI and tests.
    pub fn rex_check(&self) -> Option<&Result<crate::rex::install::Installation, String>> {
        self.rex_check
            .as_ref()
            .filter(|(location, _)| *location == self.settings.rex_location)
            .map(|(_, result)| result)
    }

    /// The local server's API address, if rexia has a local host.
    pub fn local_api_url(&self) -> Option<String> {
        self.hosts
            .iter()
            .map(|h| &h.handle.endpoint)
            .find(|e| matches!(e, Endpoint::Unix(_)))
            .map(Endpoint::to_string)
    }

    /// Allow `pattern` from the next server start, or say why it can't be.
    pub fn add_origin(&mut self, pattern: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let pattern = origin_pattern(pattern)?;
        if self.settings.web_origins.contains(&pattern) {
            return Err(format!("{pattern} is already allowed"));
        }
        self.update_settings(move |s| s.web_origins.push(pattern), cx);
        Ok(())
    }

    pub fn remove_origin(&mut self, pattern: &str, cx: &mut Context<Self>) {
        let pattern = pattern.to_owned();
        self.update_settings(move |s| s.web_origins.retain(|o| *o != pattern), cx);
    }

    /// The arguments Restart Server runs `rex` with.
    pub fn restart_args(&self) -> Vec<String> {
        server_start_args(
            &self.host().handle.endpoint.to_string(),
            &self.settings.web_origins,
        )
    }

    pub(super) fn developer_section(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let heading = |text: &str| {
            div()
                .pt(px(8.))
                .pb(px(4.))
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(text.to_owned())
                .into_any_element()
        };
        let mut rows = vec![heading("Rex Installation")];
        let custom = self.settings.rex_location.is_some();
        rows.push(
            row(
                "rex-location",
                "Location",
                "The rex that Restart Server and agent setup run: the one on PATH, or your own build, such as one in a source checkout.",
                theme,
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(button("rex-on-path", "On PATH", !custom, theme).on_click(
                        cx.listener(|ws, _, _, cx| ws.choose_rex(None, cx)),
                    ))
                    .child(button("rex-custom", "Custom", custom, theme).on_click(
                        cx.listener(|ws, _, _, cx| {
                            let text = ws.rex_field_text(cx);
                            ws.choose_rex(Some(text.unwrap_or_else(|| format!("{}/src/rex", std::env::var("HOME").unwrap_or_default())).into()), cx)
                        }),
                    )),
            )
            .into_any_element(),
        );
        if custom {
            rows.push(self.indexed_field_row(
                "rex-source",
                "Source directory",
                "A rex executable, or a checkout with one in bin, build, dist or out. Enter uses it.",
                &page.rex_source,
                theme,
                cx,
            ));
        }
        let (status, details) = match self.rex_check() {
            None => ("Checking…".to_owned(), None),
            Some(Err(why)) => (why.clone(), None),
            Some(Ok(found)) => (
                format!("Ready: rex {} at {}", found.version, found.path.display()),
                Some(found.clone()),
            ),
        };
        let ready = details.is_some();
        rows.push(
            div()
                .id("rex-status")
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(12.))
                .py(px(8.))
                .child(div().text_size(px(13.)).child("Installation status"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if ready { theme.text_muted } else { theme.error })
                        .child(status),
                )
                .into_any_element(),
        );
        if let Some(found) = details {
            for (label, value) in [
                ("Commit", found.commit.clone()),
                ("Built", found.built.clone()),
                ("SHA-256", Some(found.sha256.clone())),
            ] {
                let Some(value) = value else { continue };
                let copy = value.clone();
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(12.))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .child(
                            div()
                                .w(px(80.))
                                .flex_none()
                                .text_color(theme.text_muted)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(value),
                        )
                        .child(
                            button(format!("rex-copy-{label}"), "Copy", false, theme).on_click(
                                cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                                }),
                            ),
                        )
                        .into_any_element(),
                );
            }
        }
        rows.push(heading("Local API"));
        let url = self.local_api_url();
        let mut local = row(
            "local-api",
            "Local API URL",
            url.as_deref()
                .unwrap_or("No local host: rexia only connects to remote hosts."),
            theme,
        );
        if let Some(url) = url {
            local = local.child(button("local-api-copy", "Copy", false, theme).on_click(
                cx.listener(move |_, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                }),
            ));
        }
        rows.push(local.into_any_element());
        rows.push(heading("Experimental"));
        rows.push(self.choice_row(
            "terminal-anchor",
            "Terminal anchoring",
            "Pin terminal text to the top or the bottom of each pane; the part of a line left over goes to the other side.",
            &[
                (TerminalAnchor::Top, "Top"),
                (TerminalAnchor::Bottom, "Bottom"),
            ],
            self.settings.terminal.anchor,
            |s, v| s.terminal.anchor = v,
            theme,
            cx,
        ));
        rows.push(self.toggle_row(
            "quick-switcher",
            "New quick switcher",
            "Go to Anything lists every program reporting a status, then every session and pane.",
            |s| s.interface.quick_switcher,
            |s, v| s.interface.quick_switcher = v,
            theme,
            cx,
        ));
        rows.push(heading("Web Client Origins"));
        rows.push(
            row(
                "web-origins",
                "Allowed origins",
                if self.settings.web_origins.is_empty() {
                    "None besides Rex's own. Browsers on other sites can't open Rex streams."
                } else {
                    "Browser origins allowed to open Rex WebSocket streams, from the next Restart Server."
                },
                theme,
            )
            .into_any_element(),
        );
        for (index, origin) in self.settings.web_origins.iter().enumerate() {
            let remove = origin.clone();
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .pb(px(6.))
                    .child(div().flex_1().text_size(px(13.)).child(origin.clone()))
                    .child(
                        button(format!("origin-remove-{index}"), "Remove", false, theme).on_click(
                            cx.listener(move |ws, _, _, cx| ws.remove_origin(&remove, cx)),
                        ),
                    )
                    .into_any_element(),
            );
        }
        rows.push(Self::field_row(
            "origin-pattern",
            "Origin pattern",
            "A browser origin or wildcard subdomain, such as https://*.example.com. Enter adds it; it applies after Restart Server.",
            &page.origin,
            theme,
        ));
        if let Some(error) = &self.origin_error {
            rows.push(
                div()
                    .id("origin-error")
                    .px(px(12.))
                    .text_size(px(12.))
                    .text_color(theme.error)
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_schemes_and_hosts() {
        assert_eq!(
            origin_pattern("https://*.Example.com/").as_deref(),
            Ok("https://*.example.com")
        );
        assert_eq!(
            origin_pattern("http://localhost:5173").as_deref(),
            Ok("http://localhost:5173")
        );
        assert_eq!(
            origin_pattern("HTTPS://App.dev").as_deref(),
            Ok("https://app.dev")
        );
        assert!(origin_pattern("example.com").is_err());
        assert!(origin_pattern("https://example.com/app").is_err());
        assert!(origin_pattern("https://*").is_err());
        assert!(origin_pattern("ftp://x").is_err());
        assert_eq!(
            server_start_args("unix:///run/rex.sock", &["https://*.a.dev".into()]),
            [
                "-S",
                "unix:///run/rex.sock",
                "server",
                "start",
                "--websocket-origin-pattern",
                "https://*.a.dev"
            ]
        );
    }
}
