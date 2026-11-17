use super::{
    Workspace,
    overlay::{Choice, Item},
};
use crate::rex::{
    endpoint::Endpoint,
    forward::{self, Forward},
};
use gpui::Context;
use std::collections::BTreeMap;
use url::Url;

#[derive(Default)]
pub(super) struct Forwards {
    next: u64,
    entries: BTreeMap<u64, Entry>,
}

struct Entry {
    host: String,
    endpoint: Endpoint,
    session: String,
    target: Url,
    address: Option<Url>,
    _forward: Forward,
}

impl Workspace {
    pub(super) fn forward_items(&self, host: &str) -> Vec<Item> {
        self.forwards
            .entries
            .iter()
            .filter(|(_, entry)| entry.host == host)
            .filter_map(|(&id, entry)| {
                let address = entry.address.as_ref()?;
                Some(Item {
                    title: entry.target.to_string(),
                    detail: Some(address.to_string()),
                    keys: Some("Alt+Enter to close".into()),
                    icon: "world",
                    status: None,
                    current: false,
                    choice: Choice::OpenForward(id),
                })
            })
            .collect()
    }

    pub(super) fn forward_port(&mut self, input: &str, cx: &mut Context<Self>) {
        self.forward_on_host(&self.host().config.id.clone(), input, cx);
    }

    pub(super) fn forward_on_host(&mut self, host: &str, input: &str, cx: &mut Context<Self>) {
        let target = match forward::target(input) {
            Ok(target) => target,
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
                return;
            }
        };
        let Some(entry) = self.hosts.iter().find(|entry| entry.config.id == host) else {
            self.notice = Some("That host is no longer available".into());
            cx.notify();
            return;
        };
        let Some(session) = entry.handle.snapshot().selected else {
            self.notice = Some("Select a session to forward a port".into());
            cx.notify();
            return;
        };
        let host = host.to_owned();
        let endpoint = entry.handle.endpoint.clone();
        if let Some(entry) = self.forwards.entries.values().find(|entry| {
            entry.host == host
                && entry.endpoint == endpoint
                && entry.session == session
                && entry.target.host() == target.host()
                && entry.target.port_or_known_default() == target.port_or_known_default()
        }) {
            if let Some(address) = &entry.address {
                let mut link = target;
                let _ = link.set_host(address.host_str());
                let _ = link.set_port(address.port_or_known_default());
                (self.link_opener)(link.as_str(), cx);
            }
            return;
        }
        let (forward, ready, exited) =
            match Forward::start(endpoint.clone(), session.clone(), target.clone()) {
                Ok(worker) => worker,
                Err(error) => {
                    self.notice = Some(format!("Couldn't forward port: {error}"));
                    cx.notify();
                    return;
                }
            };
        self.forwards.next += 1;
        let id = self.forwards.next;
        self.forwards.entries.insert(
            id,
            Entry {
                host,
                endpoint,
                session,
                target,
                address: None,
                _forward: forward,
            },
        );
        cx.spawn(async move |this, cx| {
            let result = ready
                .await
                .unwrap_or_else(|_| Err("Rex did not start the forward".into()));
            let _ = this.update(cx, |ws, cx| {
                let Some(entry) = ws.forwards.entries.get_mut(&id) else {
                    return;
                };
                match result {
                    Ok(address) => {
                        ws.notice = Some(format!("Forwarded to {address}"));
                        entry.address = Some(address);
                        ws.open_forward(id, cx);
                    }
                    Err(error) => {
                        ws.forwards.entries.remove(&id);
                        ws.notice = Some(format!("Couldn't forward port: {error}"));
                    }
                }
                cx.notify();
            });
            let _ = exited.await;
            let _ = this.update(cx, |ws, cx| {
                ws.close_forward(id, cx);
            });
        })
        .detach();
    }

    pub(super) fn open_forward(&self, id: u64, cx: &mut Context<Self>) {
        if let Some(address) = self
            .forwards
            .entries
            .get(&id)
            .and_then(|entry| entry.address.as_ref())
        {
            (self.link_opener)(address.as_str(), cx);
        }
    }

    pub(super) fn notice_forward_link(&self, message: &str) -> Option<String> {
        let link = message.strip_prefix("Forwarded to ")?;
        self.forwards
            .entries
            .values()
            .any(|entry| {
                entry
                    .address
                    .as_ref()
                    .is_some_and(|address| address.as_str() == link)
            })
            .then(|| link.to_owned())
    }

    pub(super) fn close_forward(&mut self, id: u64, cx: &mut Context<Self>) {
        self.forwards.entries.remove(&id);
        cx.notify();
    }

    pub(super) fn prune_forwards(&mut self) {
        self.forwards.entries.retain(|_, entry| {
            self.hosts
                .iter()
                .any(|host| host.config.id == entry.host && host.handle.endpoint == entry.endpoint)
        });
    }
}
