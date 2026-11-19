use super::{Workspace, overlay::Overlay};
use crate::{
    rex::{
        host::HostCommand,
        prompt::{Prompt, Request},
    },
    ui::{
        icons::icon,
        input::{InputEvent, TextInput},
        theme::Theme,
    },
};
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, ScrollHandle, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::{Value, json};

pub(super) struct ScriptPrompt {
    host: String,
    request: Request,
    input: Entity<TextInput>,
    previous: Option<Overlay>,
    previous_focus: Option<FocusHandle>,
    shown: Vec<usize>,
    selected: usize,
    checked: Vec<bool>,
    scroll: ScrollHandle,
    answering: bool,
    _subscription: Subscription,
}

impl ScriptPrompt {
    fn filter(&mut self, text: &str) {
        self.shown.clear();
        if let Prompt::Pick(picker) = &self.request.prompt {
            let query = text.to_lowercase();
            self.shown
                .extend(picker.items.iter().enumerate().filter_map(|(index, item)| {
                    let fields = [
                        Some(item.title()),
                        item.subtitle.as_deref(),
                        item.detail.as_deref(),
                        Some(item.id.as_str()),
                    ];
                    fields
                        .into_iter()
                        .flatten()
                        .any(|field| field.to_lowercase().contains(&query))
                        .then_some(index)
                }));
        }
        self.selected = 0;
        self.scroll.scroll_to_item(0);
    }
}

impl Workspace {
    pub(super) fn sync_script_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Overlay::ScriptPrompt(prompt)) = &self.overlay {
            let open = self
                .hosts
                .iter()
                .find(|host| host.config.id == prompt.host)
                .is_some_and(|host| {
                    host.handle
                        .snapshot()
                        .prompt
                        .is_some_and(|request| request.call_id == prompt.request.call_id)
                });
            if open {
                return;
            }
            if let Some(Overlay::ScriptPrompt(prompt)) = self.overlay.take() {
                self.overlay = prompt.previous;
                if let Some(focus) = prompt.previous_focus {
                    window.focus(&focus, cx);
                } else {
                    self.refocus(window, cx);
                }
                cx.notify();
            }
        }
        let Some((host, request)) = self.hosts.iter().find_map(|host| {
            host.handle
                .snapshot()
                .prompt
                .map(|request| (host.config.id.clone(), request))
        }) else {
            return;
        };
        let fields = request.prompt.fields();
        let value = fields.value.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(fields.placeholder.clone(), cx);
            input.set_text(value.clone(), cx);
            input
        });
        let subscription = cx.subscribe_in(&input, window, |ws, _, event, window, cx| {
            ws.script_prompt_input(event, window, cx);
        });
        let checked = match &request.prompt {
            Prompt::Pick(picker) => picker
                .items
                .iter()
                .map(|item| picker.multiple && item.selected)
                .collect(),
            Prompt::Input(_) => Vec::new(),
        };
        let mut prompt = ScriptPrompt {
            host,
            request,
            input,
            checked,
            previous: self.overlay.take(),
            previous_focus: window.focused(cx),
            shown: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            answering: false,
            _subscription: subscription,
        };
        prompt.filter(&value);
        let focus = prompt.input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.overlay = Some(Overlay::ScriptPrompt(Box::new(prompt)));
        cx.notify();
    }

    fn script_prompt_input(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Cancel => self.close_overlay(window, cx),
            InputEvent::Changed(text) => {
                if let Some(Overlay::ScriptPrompt(prompt)) = &mut self.overlay {
                    prompt.filter(text);
                    cx.notify();
                }
            }
            InputEvent::Navigate(step) => {
                if let Some(Overlay::ScriptPrompt(prompt)) = &mut self.overlay {
                    let last = prompt.shown.len().saturating_sub(1);
                    prompt.selected = prompt
                        .selected
                        .saturating_add_signed(*step as isize)
                        .min(last);
                    prompt.scroll.scroll_to_item(prompt.selected);
                    cx.notify();
                }
            }
            InputEvent::Submit { primary, .. } => self.submit_script_prompt(*primary, window, cx),
            InputEvent::Tab => {}
        }
    }

    fn submit_script_prompt(&mut self, confirm: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::ScriptPrompt(prompt)) = &mut self.overlay else {
            return;
        };
        if prompt.answering {
            return;
        }
        let text = prompt.input.read(cx).text();
        let result = match &prompt.request.prompt {
            Prompt::Input(_) => json!({"text": text}),
            Prompt::Pick(picker) if picker.multiple && confirm => {
                let ids: Vec<_> = picker
                    .items
                    .iter()
                    .zip(&prompt.checked)
                    .filter_map(|(item, checked)| checked.then_some(&item.id))
                    .collect();
                json!({"ids": ids})
            }
            Prompt::Pick(picker) => match prompt.shown.get(prompt.selected).copied() {
                Some(index) if picker.multiple => {
                    prompt.checked[index] = !prompt.checked[index];
                    cx.notify();
                    return;
                }
                Some(index) if !confirm || !picker.allow_other => {
                    json!({"ids": [&picker.items[index].id]})
                }
                _ if picker.allow_other => json!({"text": text}),
                _ => return,
            },
        };
        self.answer_script_prompt(result, window, cx);
    }

    pub(super) fn answer_script_prompt(
        &mut self,
        result: Value,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Overlay::ScriptPrompt(prompt)) = &mut self.overlay else {
            return;
        };
        if !prompt.answering
            && let Some(host) = self.hosts.iter().find(|host| host.config.id == prompt.host)
        {
            prompt.answering = true;
            host.handle.send(HostCommand::PromptReply {
                call_id: prompt.request.call_id.clone(),
                result,
            });
            cx.notify();
        }
    }

    pub fn script_prompt_title(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::ScriptPrompt(prompt)) => Some(&prompt.request.prompt.fields().title),
            _ => None,
        }
    }

    pub(super) fn render_script_prompt(
        &self,
        prompt: &ScriptPrompt,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fields = prompt.request.prompt.fields();
        let mut body = div().flex().flex_col().gap(px(8.)).p(px(12.)).min_h_0();
        body = body
            .child(div().text_size(px(15.)).child(fields.title.clone()))
            .child(prompt.input.clone());
        if let Prompt::Pick(picker) = &prompt.request.prompt {
            let rows = prompt
                .shown
                .iter()
                .copied()
                .enumerate()
                .map(|(position, index)| {
                    let item = &picker.items[index];
                    let glyph = item.icon.as_deref().and_then(prompt_icon);
                    div()
                        .id(("script-prompt-item", position))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(theme.radius_small())
                        .cursor_pointer()
                        .when(position == prompt.selected, |row| {
                            row.bg(theme.primary_container)
                        })
                        .when(picker.multiple, |row| {
                            row.child(if prompt.checked[index] { "☑" } else { "☐" })
                        })
                        .children(
                            glyph.map(|glyph| icon(glyph).size(px(16.)).text_color(theme.text)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(div().truncate().child(item.title().to_owned()))
                                .children(item.subtitle.as_ref().map(|text| {
                                    div().truncate().text_size(px(11.)).child(text.clone())
                                })),
                        )
                        .children(
                            item.detail
                                .as_ref()
                                .map(|text| div().text_size(px(11.)).child(text.clone())),
                        )
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            if let Some(Overlay::ScriptPrompt(prompt)) = &mut ws.overlay {
                                prompt.selected = position;
                            }
                            ws.submit_script_prompt(false, window, cx);
                        }))
                });
            body = body.child(
                div()
                    .id("script-prompt-list")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&prompt.scroll)
                    .children(rows),
            );
        }
        let submit = match &prompt.request.prompt {
            Prompt::Input(_) => Some(fields.confirm.clone().unwrap_or_else(|| "Use Text".into())),
            Prompt::Pick(picker) if picker.multiple => Some(format!(
                "{} ({}) · Ctrl+Enter",
                fields.confirm.as_deref().unwrap_or("Choose"),
                prompt.checked.iter().filter(|value| **value).count()
            )),
            Prompt::Pick(picker) if picker.allow_other => {
                Some("Use Typed Text · Ctrl+Enter".into())
            }
            _ => None,
        };
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    div()
                        .id("cancel-script-prompt")
                        .px(px(10.))
                        .py(px(5.))
                        .cursor_pointer()
                        .child("Cancel")
                        .on_click(cx.listener(|ws, _, window, cx| ws.close_overlay(window, cx))),
                )
                .children(submit.map(|title| {
                    div()
                        .id("submit-script-prompt")
                        .px(px(10.))
                        .py(px(5.))
                        .rounded(theme.radius_small())
                        .bg(theme.primary)
                        .text_color(theme.background)
                        .cursor_pointer()
                        .child(title)
                        .on_click(cx.listener(|ws, _, window, cx| {
                            ws.submit_script_prompt(true, window, cx)
                        }))
                })),
        )
        .into_any_element()
    }
}

fn prompt_icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "folder" => "folder",
        "document" | "code" => "file-code",
        "branch" => "brand-git",
        "terminal" => "terminal-2",
        "server" | "database" => "server",
        "globe" => "world",
        "link" => "plug-connected",
        "gear" => "settings",
        "plus" => "plus",
        "package" => "layout-grid",
        "person" => "device-desktop",
        "tag" => "key",
        "clock" => "refresh",
        "star" => "circle-check",
        _ => return None,
    })
}
