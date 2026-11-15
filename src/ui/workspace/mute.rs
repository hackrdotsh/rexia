//! Muted tabs: a muted tab sends no desktop notifications or sounds and adds nothing to the
//! launcher badge, but keeps its status dot. Mutes are kept in settings by host, so they
//! last across launches.
use super::Workspace;
use crate::rex::host::HostModel;
use gpui::Context;
use serde_json::json;

impl Workspace {
    /// Whether the tab `window_id` on host `host` is muted.
    pub fn tab_muted(&self, host: usize, window_id: &str) -> bool {
        self.hosts.get(host).is_some_and(|entry| {
            self.settings
                .muted_tabs
                .get(&entry.config.id)
                .is_some_and(|tabs| tabs.iter().any(|t| t == window_id))
        })
    }

    /// Whether `block` sits in a muted tab.
    pub(super) fn pane_muted(&self, host: usize, block: &str, model: &HostModel) -> bool {
        let tab = model
            .views
            .values()
            .find_map(|view| view.window_of(block))
            .map(|window| window.id())
            .or_else(|| {
                self.known_tabs
                    .get(&(host, block.to_owned()))
                    .map(String::as_str)
            });
        tab.is_some_and(|tab| self.tab_muted(host, tab))
    }

    /// Remember which tab each pane is in, so a pane keeps its tab's mute while its
    /// session's layout isn't loaded. Panes that are gone are forgotten.
    pub(super) fn learn_tabs(&mut self, models: &[HostModel]) {
        for (host, model) in models.iter().enumerate() {
            for view in model.views.values() {
                for window in &view.windows {
                    for block in window.blocks().filter_map(|b| b.block_id.clone()) {
                        self.known_tabs
                            .insert((host, block), window.id().to_owned());
                    }
                }
            }
        }
        self.known_tabs.retain(|(host, block), _| {
            models
                .get(*host)
                .is_some_and(|m| m.blocks.contains_key(block))
        });
    }

    /// For hosts with muted tabs, ask the server where the panes of sessions whose layout
    /// isn't loaded sit, so their tab's mute applies before they report anything.
    pub(super) fn fetch_unknown_tabs(&mut self, cx: &mut Context<Self>) {
        for (host, entry) in self.hosts.iter().enumerate() {
            if !self.settings.muted_tabs.contains_key(&entry.config.id) {
                continue;
            }
            let model = entry.handle.snapshot();
            let sessions: std::collections::BTreeSet<String> = model
                .blocks
                .iter()
                .filter(|(block, state)| {
                    !model.views.contains_key(&state.session_id)
                        && !self.known_tabs.contains_key(&(host, (*block).clone()))
                })
                .map(|(_, state)| state.session_id.clone())
                .collect();
            for session in sessions {
                if !self.fetching_tabs.insert((host, session.clone())) {
                    continue;
                }
                let handle = entry.handle.clone();
                cx.spawn(async move |this, cx| {
                    let reply = handle
                        .request("session.view", json!({"session_id": session}))
                        .await;
                    let view = reply
                        .ok()
                        .and_then(|v| serde_json::from_value::<crate::rex::view::View>(v).ok());
                    let _ = this.update(cx, |ws, _| {
                        for window in view.iter().flat_map(|v| &v.windows) {
                            for block in window.blocks().filter_map(|b| b.block_id.clone()) {
                                ws.known_tabs.insert((host, block), window.id().to_owned());
                            }
                        }
                        // Panes the layout doesn't place (or a failed lookup) aren't asked
                        // about again; they belong to no tab.
                        if let Some(entry) = ws.hosts.get(host) {
                            for (block, state) in &entry.handle.snapshot().blocks {
                                if state.session_id == session {
                                    ws.known_tabs.entry((host, block.clone())).or_default();
                                }
                            }
                        }
                        ws.fetching_tabs.remove(&(host, session));
                    });
                })
                .detach();
            }
        }
    }

    /// Mute the tab, or unmute it if it's muted.
    pub fn toggle_tab_mute(&mut self, host: usize, window_id: String, cx: &mut Context<Self>) {
        let Some(id) = self.hosts.get(host).map(|h| h.config.id.clone()) else {
            return;
        };
        self.update_settings(
            move |s| {
                let tabs = s.muted_tabs.entry(id.clone()).or_default();
                match tabs.iter().position(|t| *t == window_id) {
                    Some(at) => {
                        tabs.remove(at);
                    }
                    None => tabs.push(window_id),
                }
                if tabs.is_empty() {
                    s.muted_tabs.remove(&id);
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Whether each tab of the shown session is muted, in order, for tests.
    pub fn tab_mutes(&self) -> Vec<bool> {
        let model = self.model();
        model
            .view()
            .map(|v| {
                v.windows
                    .iter()
                    .map(|w| self.tab_muted(self.active, w.id()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The count the launcher badge shows: unseen attention outside muted tabs.
    pub fn badge_count(&self) -> usize {
        self.badge_panes().len()
    }

    /// The panes the launcher badge counts, by block ID.
    pub fn badge_panes(&self) -> Vec<String> {
        let models: Vec<HostModel> = self.hosts.iter().map(|h| h.handle.snapshot()).collect();
        self.tracker
            .unseen()
            .filter(|(host, block)| {
                models
                    .get(*host)
                    .is_some_and(|model| !self.pane_muted(*host, block, model))
            })
            .map(|(_, block)| block.clone())
            .collect()
    }
}
