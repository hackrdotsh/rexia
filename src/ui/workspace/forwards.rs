use super::{
    Workspace,
    overlay::{Choice, Dialog, Item, Overlay},
};
use crate::rex::{
    endpoint::Endpoint,
    forward::{self, Forward, LocalPort, StartError},
};
use gpui::{Context, Window};
use std::collections::BTreeMap;
use url::Url;

#[derive(Default)]
pub(super) struct Forwards {
    next: u64,
    entries: BTreeMap<u64, Entry>,
}

struct Entry {
    request: ForwardRequest,
    address: Option<Url>,
    _forward: Forward,
}

#[derive(Clone)]
pub struct ForwardRequest {
    host: String,
    endpoint: Endpoint,
    session: String,
    target: Url,
}

impl ForwardRequest {
    pub(super) fn port(&self) -> u16 {
        self.target
            .port_or_known_default()
            .expect("validated target")
    }
}

impl Workspace {
    pub(super) fn forward_items(&self, host: &str) -> Vec<Item> {
        self.forwards
            .entries
            .iter()
            .filter(|(_, entry)| entry.request.host == host)
            .filter_map(|(&id, entry)| {
                let address = entry.address.as_ref()?;
                Some(Item {
                    title: entry.request.target.to_string(),
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
        self.start_forward(
            ForwardRequest {
                host: host.to_owned(),
                endpoint: entry.handle.endpoint.clone(),
                session,
                target,
            },
            LocalPort::Match,
            cx,
        );
    }

    fn start_forward(&mut self, request: ForwardRequest, mode: LocalPort, cx: &mut Context<Self>) {
        if !self
            .hosts
            .iter()
            .any(|host| host.config.id == request.host && host.handle.endpoint == request.endpoint)
        {
            self.notice = Some("That host is no longer available".into());
            cx.notify();
            return;
        }
        if let Some(entry) = self.forwards.entries.values().find(|entry| {
            let existing = &entry.request;
            existing.host == request.host
                && existing.endpoint == request.endpoint
                && existing.session == request.session
                && existing.target.host() == request.target.host()
                && existing.port() == request.port()
        }) {
            if let Some(address) = &entry.address {
                let mut link = request.target;
                let _ = link.set_host(address.host_str());
                let _ = link.set_port(address.port_or_known_default());
                (self.link_opener)(link.as_str(), cx);
            }
            return;
        }
        let (forward, ready, exited) = match Forward::start(
            request.endpoint.clone(),
            request.session.clone(),
            request.target.clone(),
            mode,
        ) {
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
                request,
                address: None,
                _forward: forward,
            },
        );
        cx.spawn(async move |this, cx| {
            let result = ready
                .await
                .unwrap_or_else(|_| Err(StartError::Other("Rex did not start the forward".into())));
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
                    Err(StartError::PortChoice(error)) => {
                        let request = entry.request.clone();
                        ws.forwards.entries.remove(&id);
                        ws.overlay = Some(Overlay::Dialog(Dialog::ForwardPort {
                            request,
                            error,
                            authorize: forward::authorization_available(),
                            focused: 1,
                        }));
                    }
                    Err(StartError::Other(error)) => {
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

    pub(super) fn answer_forward_port(
        &mut self,
        choice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Overlay::Dialog(Dialog::ForwardPort {
            request, authorize, ..
        })) = &self.overlay
        else {
            return;
        };
        let mode = match choice {
            0 => None,
            1 => Some(LocalPort::Free),
            2 if *authorize => Some(LocalPort::Authorize),
            _ => return,
        };
        let request = request.clone();
        self.close_overlay(window, cx);
        if let Some(mode) = mode {
            self.start_forward(request, mode, cx);
        }
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
            self.hosts.iter().any(|host| {
                host.config.id == entry.request.host
                    && host.handle.endpoint == entry.request.endpoint
            })
        });
    }
}
