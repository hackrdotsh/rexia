//! Typing a host's address in the command palette offers to add it, as Rex for Mac does:
//! `ssh://alice@studio`, `alice@studio` (an ssh login), `studio.tail1234.ts.net`, an IP
//! address or `https://…`, each with an optional port. The row opens Add Host with the
//! address filled in and a name taken from it.
use super::{
    Workspace,
    overlay::{Choice, Item, ListOverlay},
};
use crate::rex::endpoint::Endpoint;

/// The address `query` names and a name for its host, if it reads as a host's address.
pub fn typed_address(query: &str) -> Option<(String, String)> {
    let query = query.trim();
    if query.is_empty() || query.chars().any(char::is_whitespace) {
        return None;
    }
    let address = if query.contains("://") {
        query.to_owned()
    } else if query.contains('@') {
        format!("ssh://{query}")
    } else if looks_like_host(query) {
        query.to_owned()
    } else {
        return None;
    };
    let Ok(endpoint) = Endpoint::parse(&address) else {
        return None;
    };
    let host = match &endpoint {
        Endpoint::Ssh(target) => target.host.clone(),
        Endpoint::Url(url) => url.host_str()?.to_owned(),
        Endpoint::Unix(_) => return None,
    };
    if !looks_like_host(&host) && !query.contains("://") && !query.contains('@') {
        return None;
    }
    // An IP address is its own name; a host name's first label names it.
    let label = if host.parse::<std::net::IpAddr>().is_ok() {
        host
    } else {
        host.split('.').next().unwrap_or(&host).to_owned()
    };
    Some((label, address))
}

/// A dotted host name or IP address, with an optional port: `studio.example`,
/// `10.0.0.2:8080`. Single words don't count; they're far likelier to be a search.
fn looks_like_host(text: &str) -> bool {
    let name = match text.rsplit_once(':') {
        Some((name, port)) if port.parse::<u16>().is_ok() => name,
        _ => text,
    };
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        // A top-level label of letters, or all numbers (an IPv4 address).
        && (labels.last().is_some_and(|l| l.len() >= 2 && l.chars().all(|c| c.is_ascii_alphabetic()))
            || name.parse::<std::net::Ipv4Addr>().is_ok())
}

impl Workspace {
    /// The name and address an open Add Host dialog holds, for tests.
    pub fn add_host_fields(&self, cx: &gpui::App) -> Option<(String, String)> {
        match &self.overlay {
            Some(super::overlay::Overlay::Dialog(super::overlay::Dialog::AddHost {
                label,
                address,
                ..
            })) => Some((
                label.read(cx).text().to_owned(),
                address.read(cx).text().to_owned(),
            )),
            _ => None,
        }
    }

    /// The command palette's query changed: offer to add the host it names, last.
    pub(super) fn offer_typed_host(list: &mut ListOverlay, query: &str, known: &[String]) {
        list.items
            .retain(|i| !matches!(i.choice, Choice::AddTypedHost { .. }));
        list.shown.retain(|i| *i < list.items.len());
        let Some((label, address)) = typed_address(query) else {
            return;
        };
        let same = |known: &String| {
            let known = known.trim().trim_end_matches('/');
            known.eq_ignore_ascii_case(address.trim_end_matches('/'))
                || known.eq_ignore_ascii_case(query.trim())
        };
        if known.iter().any(same) {
            return;
        }
        list.items.push(Item {
            title: format!("Add Host “{}”…", query.trim()),
            detail: Some("Remote Hosts".into()),
            keys: None,
            icon: "plus",
            status: None,
            current: false,
            choice: Choice::AddTypedHost { label, address },
        });
        list.shown.push(list.items.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::typed_address;

    #[test]
    fn addresses_are_told_from_searches() {
        let named = typed_address;
        assert_eq!(
            named("alice@studio"),
            Some(("studio".into(), "ssh://alice@studio".into()))
        );
        assert_eq!(
            named("ssh://alice@studio.local:2222"),
            Some(("studio".into(), "ssh://alice@studio.local:2222".into()))
        );
        assert_eq!(
            named("studio.tail1234.ts.net"),
            Some(("studio".into(), "studio.tail1234.ts.net".into()))
        );
        assert_eq!(
            named("https://rex.example.com"),
            Some(("rex".into(), "https://rex.example.com".into()))
        );
        assert_eq!(
            named("10.0.0.2:8443"),
            Some(("10.0.0.2".into(), "10.0.0.2:8443".into()))
        );
        for search in [
            "split",
            "new tab",
            "studio",
            "v1.2",
            "pane.split2",
            "",
            "a.b",
        ] {
            assert_eq!(named(search), None, "{search:?} reads as a search");
        }
    }
}
