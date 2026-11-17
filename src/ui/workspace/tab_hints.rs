//! Hold the modifier of the Go to Tab keys (Alt, unless the keymap says otherwise) to see
//! each tab's key on it, as Rex for Mac shows them while Command is held. The hints wait a
//! moment so Alt+key combinations typed in a terminal don't flash them, go away at any
//! other key or when the modifier is let go, and follow the keymap: a tab whose number has
//! no key gets no hint.
use super::Workspace;
use crate::{keymap, ui::theme::Theme};
use gpui::{AnyElement, Context, IntoElement, Modifiers, ParentElement, Styled, div, px};
use serde_json::json;
use std::time::Duration;

/// How long the modifier is held, alone, before the hints show.
const DELAY: Duration = Duration::from_millis(400);

impl Workspace {
    /// The key that goes to tab `number` (1 to 9), as shown, when it's one stroke.
    pub fn tab_hint(&self, number: usize) -> Option<String> {
        let keys = self
            .keymap
            .keys_for("client.tab.goto", Some(&json!({"index": number})))?;
        (keys.len() == 1).then(|| keymap::display(keys))
    }

    /// The modifiers the Go to Tab keys use, from tab 1's key.
    fn tab_hint_modifiers(&self) -> Option<Modifiers> {
        let keys = self
            .keymap
            .keys_for("client.tab.goto", Some(&json!({"index": 1})))?;
        let [key] = keys else { return None };
        let mut modifiers = Modifiers::default();
        let parts: Vec<&str> = key.split('+').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            match *part {
                "ctrl" => modifiers.control = true,
                "alt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "cmd" | "super" => modifiers.platform = true,
                _ => return None,
            }
        }
        (modifiers != Modifiers::default()).then_some(modifiers)
    }

    /// The modifier keys changed: start waiting to show the hints, or hide them.
    pub(super) fn tab_hint_modifiers_changed(&mut self, now: Modifiers, cx: &mut Context<Self>) {
        let holding = self
            .tab_hint_modifiers()
            .is_some_and(|wanted| wanted == now);
        if !holding {
            self.cancel_tab_hints(cx);
            return;
        }
        if self.tab_hint_wait.is_some() || self.tab_hints_shown {
            return;
        }
        self.tab_hint_generation += 1;
        let generation = self.tab_hint_generation;
        self.tab_hint_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DELAY).await;
            let _ = this.update(cx, |ws, cx| {
                if ws.tab_hint_generation == generation {
                    ws.tab_hint_wait = None;
                    ws.tab_hints_shown = true;
                    cx.notify();
                }
            });
        }));
    }

    /// Another key, or the modifier let go: no hints.
    pub(super) fn cancel_tab_hints(&mut self, cx: &mut Context<Self>) {
        self.tab_hint_generation += 1;
        self.tab_hint_wait = None;
        if std::mem::take(&mut self.tab_hints_shown) {
            cx.notify();
        }
    }

    /// The hints showing on the tabs, in order, for tests.
    pub fn tab_hints(&self) -> Vec<Option<String>> {
        if !self.tab_hints_shown {
            return Vec::new();
        }
        let count = self.model().view().map_or(0, |v| v.windows.len());
        (1..=count).map(|n| self.tab_hint(n)).collect()
    }

    /// The chip with tab `number`'s key, while the hints show; `on_accent` for a tab
    /// filled with the accent color, where the chip turns the other way round.
    pub(super) fn tab_hint_chip(
        &self,
        number: usize,
        on_accent: bool,
        theme: &Theme,
    ) -> Option<AnyElement> {
        if !self.tab_hints_shown {
            return None;
        }
        let key = self.tab_hint(number)?;
        Some(
            div()
                .flex_none()
                .px(px(5.))
                .rounded(theme.radius_small())
                .bg(if on_accent {
                    theme.on_primary
                } else {
                    theme.primary
                })
                .text_color(if on_accent {
                    theme.primary
                } else {
                    theme.on_primary
                })
                .text_size(px(11.))
                .child(key)
                .into_any_element(),
        )
    }
}
