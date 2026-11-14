//! Desktop notifications through the freedesktop service (Noctalia's, on a Noctalia desktop),
//! and the unseen-activity count launchers show on rexia's icon.
use std::collections::HashMap;
use zbus::{Connection, proxy, zvariant::Value};

const APP_NAME: &str = "rexia";
const APP_ID: &str = "dev.hackr.rexia";

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: &str) -> zbus::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    Low = 0,
    Normal = 1,
    Critical = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// What clicking the notification shows, such as a block ID. A newer note with the same
    /// key replaces an older one still on screen.
    pub key: String,
    pub summary: String,
    pub body: String,
    pub urgency: Urgency,
}

enum Message {
    Show(Note),
    Withdraw(String),
    Badge(u32),
}

/// Sends notifications from its own thread, so a slow or missing service never stalls the UI.
#[derive(Clone)]
pub struct Notifier {
    messages: flume::Sender<Message>,
}

impl Notifier {
    /// Connect to the session bus (or `address`); clicked notifications' keys arrive on the
    /// returned receiver. Without a bus, notes are dropped quietly.
    pub fn spawn(address: Option<String>) -> (Self, flume::Receiver<String>) {
        let (messages, received) = flume::unbounded();
        let (clicks, clicked) = flume::unbounded();
        std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("notification runtime");
                runtime.block_on(async move {
                    if let Err(error) = run(address, received, clicks).await {
                        eprintln!("rexia: desktop notifications are off: {error:#}");
                    }
                });
            })
            .expect("start notification thread");
        (Self { messages }, clicked)
    }

    pub fn show(&self, note: Note) {
        let _ = self.messages.send(Message::Show(note));
    }

    /// Close the notification for `key`, once what it reported has been seen.
    pub fn withdraw(&self, key: impl Into<String>) {
        let _ = self.messages.send(Message::Withdraw(key.into()));
    }

    /// The number launchers and docks show on rexia's icon; 0 hides it.
    pub fn badge(&self, count: u32) {
        let _ = self.messages.send(Message::Badge(count));
    }
}

async fn run(
    address: Option<String>,
    messages: flume::Receiver<Message>,
    clicks: flume::Sender<String>,
) -> anyhow::Result<()> {
    let connection = match address {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())?
                .build()
                .await?
        }
        None => Connection::session().await?,
    };
    let proxy = NotificationsProxy::new(&connection).await?;
    let ids: std::sync::Arc<std::sync::Mutex<HashMap<String, u32>>> = Default::default();

    let mut invoked = proxy.receive_action_invoked().await?;
    let click_ids = ids.clone();
    tokio::spawn(async move {
        use futures_util::StreamExt;
        while let Some(signal) = invoked.next().await {
            let Ok(args) = signal.args() else { continue };
            let key = click_ids
                .lock()
                .expect("notification ids")
                .iter()
                .find(|(_, id)| **id == args.id)
                .map(|(key, _)| key.clone());
            if let Some(key) = key {
                let _ = clicks.send(key);
            }
        }
    });

    let mut last_badge = None;
    while let Ok(message) = messages.recv_async().await {
        match message {
            Message::Show(note) => {
                let replaces = ids
                    .lock()
                    .expect("notification ids")
                    .get(&note.key)
                    .copied()
                    .unwrap_or(0);
                let mut hints = HashMap::new();
                hints.insert("desktop-entry", Value::from(APP_ID));
                hints.insert("urgency", Value::U8(note.urgency as u8));
                let result = proxy
                    .notify(
                        APP_NAME,
                        replaces,
                        APP_ID,
                        &note.summary,
                        &note.body,
                        &["default", "Show"],
                        hints,
                        -1,
                    )
                    .await;
                match result {
                    Ok(id) => {
                        ids.lock().expect("notification ids").insert(note.key, id);
                    }
                    Err(error) => eprintln!("rexia: couldn't show a notification: {error}"),
                }
            }
            Message::Withdraw(key) => {
                let id = ids.lock().expect("notification ids").remove(&key);
                if let Some(id) = id {
                    let _ = proxy.close_notification(id).await;
                }
            }
            Message::Badge(count) => {
                if last_badge == Some(count) {
                    continue;
                }
                last_badge = Some(count);
                let mut properties: HashMap<&str, Value<'_>> = HashMap::new();
                properties.insert("count", Value::I64(count as i64));
                properties.insert("count-visible", Value::Bool(count > 0));
                // Docks that follow the Unity launcher API show this on rexia's icon.
                let _ = connection
                    .emit_signal(
                        None::<()>,
                        "/dev/hackr/rexia",
                        "com.canonical.Unity.LauncherEntry",
                        "Update",
                        &(format!("application://{APP_ID}.desktop"), properties),
                    )
                    .await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Child, Command, Stdio},
        sync::{Arc, Mutex},
        time::Duration,
    };

    /// A private bus, so tests never reach the real desktop.
    struct Bus {
        child: Child,
        address: String,
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn private_bus() -> Option<Bus> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(child.stdout.take()?),
            &mut line,
        )
        .ok()?;
        Some(Bus {
            child,
            address: line.trim().to_owned(),
        })
    }

    /// A shown notification: its ID, summary, body and urgency.
    type Shown = (u32, String, String, u8);

    #[derive(Clone, Default)]
    struct Fake {
        shown: Arc<Mutex<Vec<Shown>>>,
        closed: Arc<Mutex<Vec<u32>>>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Fake {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app_name: &str,
            replaces_id: u32,
            _app_icon: &str,
            summary: &str,
            body: &str,
            _actions: Vec<String>,
            hints: HashMap<String, zbus::zvariant::OwnedValue>,
            _expire_timeout: i32,
        ) -> u32 {
            let urgency = hints
                .get("urgency")
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(9);
            let mut shown = self.shown.lock().unwrap();
            let id = if replaces_id == 0 {
                shown.len() as u32 + 1
            } else {
                replaces_id
            };
            shown.push((id, summary.to_owned(), body.to_owned(), urgency));
            id
        }

        fn close_notification(&self, id: u32) {
            self.closed.lock().unwrap().push(id);
        }

        #[zbus(signal)]
        async fn action_invoked(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            id: u32,
            action_key: &str,
        ) -> zbus::Result<()>;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn shows_replaces_withdraws_and_reports_clicks() {
        let Some(bus) = private_bus() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let fake = Fake::default();
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", fake.clone())
            .unwrap()
            .build()
            .await
            .unwrap();

        let (notifier, clicks) = Notifier::spawn(Some(bus.address.clone()));
        let note = |summary: &str| Note {
            key: "block:1".into(),
            summary: summary.into(),
            body: "claude".into(),
            urgency: Urgency::Critical,
        };
        notifier.show(note("Needs approval"));
        notifier.show(note("Done"));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while fake.shown.lock().unwrap().len() < 2 {
            assert!(std::time::Instant::now() < deadline, "notes never arrived");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let shown = fake.shown.lock().unwrap().clone();
        assert_eq!(shown[0], (1, "Needs approval".into(), "claude".into(), 2));
        assert_eq!(
            shown[1].0, 1,
            "the second note for the same pane replaces the first"
        );

        // Clicking the notification reports the pane it was about.
        let emitter =
            zbus::object_server::SignalEmitter::new(&server, "/org/freedesktop/Notifications")
                .unwrap();
        Fake::action_invoked(&emitter, 1, "default").await.unwrap();
        let clicked = clicks.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(clicked, "block:1");

        notifier.withdraw("block:1");
        while fake.closed.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "withdraw never arrived"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(*fake.closed.lock().unwrap(), vec![1]);
    }
}
