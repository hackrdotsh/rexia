//! Search, direct links and resets share the same client-settings index.
use super::{
    Workspace,
    overlay::Overlay,
    preferences::{self, Section, SettingsPage},
};
use crate::{
    settings_index::{self as index, Entry},
    ui::{input::InputEvent, theme::Theme},
};
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, Focusable, IntoElement, ParentElement, Styled,
    Window, div, prelude::*, px,
};
use serde_json::{Value, json};

fn section(title: &str) -> Section {
    Section::ALL
        .into_iter()
        .find(|s| s.title() == title)
        .unwrap_or(Section::Interface)
}

fn input_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        value => value.to_string(),
    }
}

/// A settings URI can name a section, a row ID, or its full serialized path.
pub fn destination(link: &str) -> Result<(Section, Option<Entry>), String> {
    let url = url::Url::parse(link).map_err(|e| e.to_string())?;
    if url.scheme() != "rex" || url.host_str() != Some("settings") {
        return Err("Not a settings link".into());
    }
    let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Ok((Section::Interface, None));
    }
    let dotted = parts.join(".").replace('-', "_");
    if let Some(entry) = index::find(&dotted).or_else(|| index::find(parts.last().unwrap())) {
        return Ok((section(entry.section), Some(entry)));
    }
    let target = match parts[0] {
        "interface" => Section::Interface,
        "terminal" => Section::Terminal,
        "shortcuts" => Section::Shortcuts,
        "notifications" => Section::Notifications,
        "hosts" | "client" | "remote-hosts" => Section::Hosts,
        "server" => Section::Server,
        "integrations" => Section::Integrations,
        "developer" => Section::Developer,
        other => return Err(format!("Unknown settings section: {other}")),
    };
    // Group links land in their owning section. A row ID at the end is resolved above.
    Ok((target, None))
}

impl Workspace {
    pub fn settings_target(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::Settings(page)) => page.setting_target.as_deref(),
            _ => None,
        }
    }

    pub fn setting_error(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::Settings(page)) => page.setting_error.as_deref(),
            _ => None,
        }
    }

    pub fn settings_group(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::Settings(page)) => page.setting_group.as_deref(),
            _ => None,
        }
    }

    pub fn open_indexed_setting(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entry = index::find(path).ok_or("Unknown setting")?;
        self.commit_settings_fields(cx);
        if !matches!(self.overlay, Some(Overlay::Settings(_))) {
            self.open_settings(window, cx);
        }
        self.show_settings_section(section(entry.section), window, cx);
        let value = index::value(&self.settings, &entry.path);
        if let Some(Overlay::Settings(page)) = &mut self.overlay {
            page.setting_target = Some(entry.path.clone());
            page.setting_error = None;
            page.setting_value
                .update(cx, |input, cx| input.set_text(input_text(&value), cx));
            if !value.is_boolean() && index::choices(&entry.path).is_empty() {
                window.focus(&page.setting_value.read(cx).focus_handle(cx), cx);
            } else {
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
        Ok(())
    }

    pub fn open_settings_link(
        &mut self,
        link: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (section, entry) = destination(link)?;
        if let Some(entry) = entry {
            return self.open_indexed_setting(&entry.path, window, cx);
        }
        self.commit_settings_fields(cx);
        if !matches!(self.overlay, Some(Overlay::Settings(_))) {
            self.open_settings(window, cx);
        }
        self.show_settings_section(section, window, cx);
        let url = url::Url::parse(link).map_err(|e| e.to_string())?;
        let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() > 1 {
            let group = parts.last().unwrap().to_string();
            if !index::entries()
                .iter()
                .any(|entry| entry.section == section.title() && entry.group == group)
            {
                return Err(format!("Unknown settings group: {group}"));
            }
            if let Some(Overlay::Settings(page)) = &mut self.overlay {
                page.setting_group = Some(group);
            }
        }
        Ok(())
    }

    pub fn update_indexed_setting(
        &mut self,
        path: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entry = index::find(path).ok_or("Unknown setting")?;
        let path = entry.path.as_str();
        let settings = index::set(&self.settings, path, value)?;
        let label = settings.client_label.clone();
        self.update_settings(|current| *current = settings, cx);
        if path == "client_label" {
            self.identity.label = label.clone();
            for host in &self.hosts {
                host.handle
                    .send(crate::rex::host::HostCommand::SetLabel(label.clone()));
            }
        }
        if path == "ssh_agent" {
            self.choose_ssh_agent(self.settings.ssh_agent.clone(), cx);
        }
        if path == "rex_location" {
            self.choose_rex(self.settings.rex_location.clone(), cx);
        }
        self.sync_indexed_field(path, cx);
        Ok(())
    }

    fn sync_indexed_field(&mut self, path: &str, cx: &mut Context<Self>) {
        let value = index::value(&self.settings, path);
        let Some(Overlay::Settings(page)) = &mut self.overlay else {
            return;
        };
        let field = match path {
            "client_label" => Some(&page.label),
            "terminal.font_family" => Some(&page.font),
            "terminal.fallback_fonts" => Some(&page.fallbacks),
            "terminal.startup_command" => Some(&page.startup),
            "ssh_agent" => Some(&page.ssh_socket),
            "rex_location" => Some(&page.rex_source),
            _ => None,
        };
        if let Some(field) = field {
            field.update(cx, |input, cx| input.set_text(input_text(&value), cx));
        }
        if page.setting_target.as_deref() == Some(path) {
            page.setting_value
                .update(cx, |input, cx| input.set_text(input_text(&value), cx));
            page.setting_error = None;
        }
    }

    pub fn reset_indexed_setting(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entry = index::find(path).ok_or("Unknown setting")?;
        self.update_indexed_setting(&entry.path, entry.default, cx)
    }

    pub(super) fn reset_setting_control(
        &self,
        id: &str,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let entries = if let Some(state) = id.strip_prefix("state-") {
            let prefix = format!("notifications.{}.", state.to_lowercase());
            index::entries()
                .into_iter()
                .filter(|entry| entry.path.starts_with(&prefix))
                .collect::<Vec<_>>()
        } else {
            vec![index::find(id)?]
        };
        if !entries
            .iter()
            .any(|entry| index::changed(&self.settings, entry))
        {
            return None;
        }
        Some(
            preferences::button(format!("reset-{id}"), "Reset", false, theme)
                .relative()
                .child(self.track_bounds(format!("setting-reset:{id}"), cx))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    for entry in &entries {
                        if let Err(error) = ws.reset_indexed_setting(&entry.path, cx) {
                            ws.notice = Some(error);
                        }
                    }
                }))
                .into_any_element(),
        )
    }

    pub(super) fn indexed_field_row(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        input: &Entity<crate::ui::input::TextInput>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        preferences::row(id, title, detail, theme)
            .child(div().w(px(260.)).flex_none().child(input.clone()))
            .children(self.reset_setting_control(id, theme, cx))
            .into_any_element()
    }

    pub(super) fn settings_search_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Overlay::Settings(page)) = &mut self.overlay else {
            return;
        };
        let query = page.search.read(cx).text().to_owned();
        let mut results = index::search(&query);
        if let Some(group) = &page.setting_group {
            results.retain(|entry| entry.group == group && entry.section == page.section.title());
        }
        match event {
            InputEvent::Changed(_) => {
                page.search_selected = 0;
                if !query.is_empty() {
                    page.setting_group = None;
                }
            }
            InputEvent::Navigate(step) => {
                if !results.is_empty() {
                    page.search_selected = (page.search_selected as i32 + step)
                        .rem_euclid(results.len() as i32)
                        as usize;
                }
            }
            InputEvent::Submit { .. } => {
                if let Some(entry) = results.get(page.search_selected) {
                    let path = entry.path.clone();
                    let _ = self.open_indexed_setting(&path, window, cx);
                }
            }
            InputEvent::Cancel => {
                page.search.update(cx, |input, cx| input.set_text("", cx));
                window.focus(&self.focus, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn submit_setting_value(&mut self, cx: &mut Context<Self>) {
        let Some(Overlay::Settings(page)) = &self.overlay else {
            return;
        };
        let Some(path) = page.setting_target.clone() else {
            return;
        };
        let Some(entry) = index::find(&path) else {
            return;
        };
        let text = page.setting_value.read(cx).text().trim().to_owned();
        let value = if text.is_empty() && entry.default.is_null() {
            Ok(Value::Null)
        } else if index::number_range(&path).is_some() {
            serde_json::from_str(&text).map_err(|_| "Enter a number".to_owned())
        } else if entry.default.is_array() {
            Ok(json!(
                text.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            ))
        } else {
            Ok(json!(text))
        };
        let result = value.and_then(|value| self.update_indexed_setting(&path, value, cx));
        if let Err(error) = result
            && let Some(Overlay::Settings(page)) = &mut self.overlay
        {
            page.setting_error = Some(error);
        }
        cx.notify();
    }

    pub(super) fn settings_search_rows(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut results = index::search(page.search.read(cx).text());
        if let Some(group) = &page.setting_group {
            results.retain(|entry| entry.group == group && entry.section == page.section.title());
        }
        if results.is_empty() {
            return vec![
                div()
                    .py(px(16.))
                    .child("No matching settings")
                    .into_any_element(),
            ];
        }
        results
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let value = index::display(&index::value(&self.settings, &entry.path));
                preferences::row(
                    format!("setting-result-{}", entry.id),
                    &entry.title,
                    entry.section,
                    theme,
                )
                .when(i == page.search_selected, |d| d.bg(theme.primary_container))
                .child(value)
                .cursor_pointer()
                .on_click(cx.listener(move |ws, _, window, cx| {
                    let _ = ws.open_indexed_setting(&entry.path, window, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn setting_editor_rows(
        &self,
        page: &SettingsPage,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(entry) = page.setting_target.as_deref().and_then(index::find) else {
            return vec![];
        };
        let value = index::value(&self.settings, &entry.path);
        let options = index::choices(&entry.path);
        let mut control = div().flex().flex_col().gap(px(12.)).py(px(12.));
        if value.is_boolean() {
            let on = value.as_bool().unwrap_or(false);
            let path = entry.path.clone();
            control = control.child(
                preferences::button("setting-toggle", if on { "On" } else { "Off" }, on, theme)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        let _ = ws.update_indexed_setting(&path, json!(!on), cx);
                    })),
            );
        } else if !options.is_empty() {
            let choices = options.into_iter().enumerate().map(|(i, option)| {
                let path = entry.path.clone();
                preferences::button(
                    format!("setting-option-{i}"),
                    &index::choice_label(&option),
                    option == value,
                    theme,
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    let _ = ws.update_indexed_setting(&path, option.clone(), cx);
                }))
            });
            control = control.child(div().flex().flex_wrap().gap(px(6.)).children(choices));
        } else {
            control = control.child(page.setting_value.clone()).child(
                preferences::button("setting-apply", "Apply", true, theme)
                    .on_click(cx.listener(|ws, _, _, cx| ws.submit_setting_value(cx))),
            );
        }
        let uri = format!("rex://settings/{}", entry.path.replace('.', "/"));
        vec![
            preferences::row("focused-setting", &entry.title, &entry.path, theme)
                .children(self.reset_setting_control(&entry.path, theme, cx))
                .into_any_element(),
            control.into_any_element(),
            div()
                .children(
                    page.setting_error
                        .clone()
                        .map(|error| div().text_color(theme.error).child(error)),
                )
                .into_any_element(),
            preferences::button("copy-setting-link", "Copy link", false, theme)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(uri.clone()))
                }))
                .into_any_element(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_links_find_sections_and_rows() {
        assert_eq!(
            destination("rex://settings/server").unwrap().0,
            Section::Server
        );
        assert_eq!(
            destination("rex://settings/terminal/typography/font-size")
                .unwrap()
                .1
                .unwrap()
                .path,
            "terminal.font_size"
        );
        assert_eq!(
            destination("rex://settings/notifications/sounds")
                .unwrap()
                .1
                .unwrap()
                .path,
            "notifications.sounds"
        );
        assert!(destination("rex://settings/unknown").is_err());
    }
}
