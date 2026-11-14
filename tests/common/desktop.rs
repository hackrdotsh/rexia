//! A private session bus with a fake notification server, so tests see what rexia would show
//! on the desktop without ever reaching the real one.
use std::{
    collections::HashMap,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zbus::object_server::SignalEmitter;

#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub urgency: u8,
}

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
            shown.iter().map(|s| s.id).max().unwrap_or(0) + 1
        } else {
            replaces_id
        };
        shown.push(Shown {
            id,
            summary: summary.to_owned(),
            body: body.to_owned(),
            urgency,
        });
        id
    }

    fn close_notification(&self, id: u32) {
        self.closed.lock().unwrap().push(id);
    }

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

pub struct Desktop {
    pub address: String,
    fake: Fake,
    connection: Option<zbus::Connection>,
    runtime: tokio::runtime::Runtime,
    bus: Child,
}

impl Desktop {
    /// Start the bus and the fake server, or `None` without dbus-daemon.
    pub fn start() -> Option<Self> {
        let mut bus = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(bus.stdout.take()?), &mut line)
            .ok()?;
        let address = line.trim().to_owned();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .ok()?;
        let fake = Fake::default();
        let connection = runtime
            .block_on(
                zbus::connection::Builder::address(address.as_str())
                    .ok()?
                    .name("org.freedesktop.Notifications")
                    .ok()?
                    .serve_at("/org/freedesktop/Notifications", fake.clone())
                    .ok()?
                    .build(),
            )
            .ok()?;
        Some(Self {
            address,
            fake,
            connection: Some(connection),
            runtime,
            bus,
        })
    }

    pub fn shown(&self) -> Vec<Shown> {
        self.fake.shown.lock().unwrap().clone()
    }

    pub fn closed(&self) -> Vec<u32> {
        self.fake.closed.lock().unwrap().clone()
    }

    /// Click a notification, as the user would.
    pub fn click(&self, id: u32) {
        let connection = self.connection.clone().unwrap();
        self.runtime.block_on(async move {
            let emitter =
                SignalEmitter::new(&connection, "/org/freedesktop/Notifications").unwrap();
            Fake::action_invoked(&emitter, id, "default").await.unwrap();
        });
    }

    /// Wait until a shown notification satisfies `check`, calling `pump` meanwhile.
    pub fn wait_shown(
        &self,
        what: &str,
        mut pump: impl FnMut(),
        check: impl Fn(&Shown) -> bool,
    ) -> Shown {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            pump();
            if let Some(found) = self.shown().into_iter().find(|s| check(s)) {
                return found;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        drop(self.connection.take());
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}
