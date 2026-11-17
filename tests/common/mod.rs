//! Shared harness for tests that drive a headless rexia workspace against a real Rex server
//! named by REXIA_TEST_SERVER.
#![allow(dead_code)]
pub mod desktop;

use gpui::{
    AnyWindowHandle, AppContext as _, HeadlessAppContext, Keystroke, WindowHandle, px, size,
};
use rexia::{
    noctalia,
    rex::{
        client::Identity,
        endpoint::Endpoint,
        host::{Connection, HostCommand, HostHandle, HostModel},
    },
    settings::{HostConfig, Settings},
    ui::{icons::Assets, theme::Theme, workspace::Workspace},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub fn server() -> Option<String> {
    let address = std::env::var("REXIA_TEST_SERVER").ok();
    if address.is_none() {
        eprintln!("skipped: set REXIA_TEST_SERVER to a disposable Rex server");
    }
    address
}

/// A second client, as another device or the CLI would be.
pub fn connect(address: &str) -> HostHandle {
    let host = HostHandle::spawn(Endpoint::parse(address).unwrap(), Identity::default(), None);
    let deadline = Instant::now() + Duration::from_secs(20);
    while host.snapshot().connection != Connection::Connected {
        assert!(Instant::now() < deadline, "couldn't connect to {address}");
        let _ = host.wake.recv_timeout(Duration::from_millis(50));
    }
    host
}

pub fn request(host: &HostHandle, method: &str, payload: Value) -> Result<Value, String> {
    let (reply, answer) = tokio::sync::oneshot::channel();
    host.send(HostCommand::Call {
        method: method.into(),
        payload,
        reply: Some(reply),
    });
    answer.blocking_recv().unwrap()
}

/// Destroys a test's sessions even when an assertion fails partway.
pub struct Sessions {
    pub host: HostHandle,
    pub ids: Vec<String>,
}

impl Sessions {
    pub fn create(host: &HostHandle, label: &str) -> Self {
        let mut sessions = Self {
            host: host.clone(),
            ids: Vec::new(),
        };
        sessions.add(label);
        sessions
    }

    pub fn add(&mut self, label: &str) -> String {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.host.send(HostCommand::CreateSession {
            label: Some(label.into()),
            reply: Some(reply),
        });
        let id = answer.blocking_recv().unwrap().unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.ids.push(id.clone());
        id
    }

    /// Remember a session something else created, so it's cleaned up too.
    pub fn track(&mut self, id: String) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }
}

impl Drop for Sessions {
    fn drop(&mut self) {
        for id in &self.ids {
            let _ = request(&self.host, "session.destroy", json!({"session_id": id}));
        }
    }
}

/// The text of the focused pane on the shown tab.
pub fn focused_text(model: &HostModel) -> String {
    let Some(block) = model
        .view()
        .and_then(|v| v.active_window()?.focused_block_id.clone())
    else {
        return String::new();
    };
    model
        .panes
        .get(&block)
        .and_then(|p| p.snapshot().frame)
        .map(|f| {
            (0..f.rows as usize)
                .map(|r| f.line_text(r))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

pub fn shot_dir() -> PathBuf {
    let dir = std::env::var_os("REXIA_SHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Settings with one host, `test`, at `address`, showing `session`.
pub fn test_settings(address: &str, session: &str) -> Settings {
    Settings {
        hosts: vec![HostConfig {
            id: "test".into(),
            label: "flareon".into(),
            endpoint: address.into(),
        }],
        last_session: Some(session.into()),
        ..Default::default()
    }
}

pub struct Harness {
    pub cx: HeadlessAppContext,
    pub window: WindowHandle<Workspace>,
    pub host: HostHandle,
}

impl Harness {
    /// A workspace window attached to `address`, showing `session`.
    pub fn open(address: &str, session: &str, width: f32, height: f32) -> Self {
        Self::open_with(test_settings(address, session), width, height)
    }

    /// A workspace window with these settings, as rexia launched with them would be.
    pub fn open_with(settings: Settings, width: f32, height: f32) -> Self {
        Self::open_with_path(settings, None, width, height)
    }

    pub fn open_with_path(
        settings: Settings,
        path: Option<PathBuf>,
        width: f32,
        height: f32,
    ) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
            Arc::new(Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| cx.set_global(Theme::from_noctalia(&noctalia::load())));
        let window = cx
            .open_window(size(px(width), px(height)), |window, cx| {
                cx.new(|cx| Workspace::new(settings, path, false, window, cx))
            })
            .unwrap();
        let host = cx.update(|cx| window.read(cx).unwrap().host().handle.clone());
        Self { cx, window, host }
    }

    pub fn pump(&mut self) {
        let workspace = self.window;
        self.cx.update(|cx| {
            workspace
                .update(cx, |ws, window, cx| {
                    ws.sync(window, cx);
                    window.refresh();
                })
                .unwrap()
        });
        self.cx.run_until_parked();
    }

    pub fn wait<T>(&mut self, what: &str, mut check: impl FnMut(&HostModel) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.pump();
            if let Some(value) = check(&self.host.snapshot()) {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            let _ = self.host.wake.recv_timeout(Duration::from_millis(50));
        }
    }

    /// Wait for something the workspace itself shows, such as an open palette.
    pub fn wait_ui<T>(&mut self, what: &str, mut check: impl FnMut(&Workspace) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.pump();
            if let Some(value) = self.workspace(&mut check) {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Press keys written as GPUI spells them, separated by spaces: `ctrl-a c`.
    pub fn press(&mut self, keys: &str) {
        for key in keys.split(' ') {
            let key: &'static str = Box::leak(key.to_owned().into_boxed_str());
            let mut keystroke = Keystroke::parse(key).unwrap();
            if keystroke.key.chars().count() == 1 && !keystroke.modifiers.control {
                keystroke.key_char = Some(keystroke.key.clone());
            }
            self.dispatch(keystroke);
        }
    }

    /// Type text as a keyboard would.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let (key, shift) = match c {
                ' ' => ("space".to_owned(), false),
                c if c.is_ascii_uppercase() => (c.to_ascii_lowercase().to_string(), true),
                c => (c.to_string(), false),
            };
            self.dispatch(Keystroke {
                modifiers: gpui::Modifiers {
                    shift,
                    ..Default::default()
                },
                key,
                key_char: Some(c.to_string()),
            });
        }
    }

    /// Send a raw input event to the window, as the platform would.
    #[allow(dead_code)]
    pub fn input(&mut self, event: gpui::PlatformInput) {
        let handle: AnyWindowHandle = self.window.into();
        self.cx
            .update_window(handle, |_, window, cx| {
                window.dispatch_event(event, cx);
            })
            .unwrap();
        self.pump();
    }

    fn dispatch(&mut self, keystroke: Keystroke) {
        let handle: AnyWindowHandle = self.window.into();
        self.cx
            .update_window(handle, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .unwrap();
        self.pump();
    }

    /// Call a method from another client while rexia keeps running, as it would.
    pub fn call_from(
        &mut self,
        other: &HostHandle,
        method: &str,
        payload: Value,
    ) -> Result<Value, String> {
        let (reply, mut answer) = tokio::sync::oneshot::channel();
        other.send(HostCommand::Call {
            method: method.into(),
            payload,
            reply: Some(reply),
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.pump();
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            assert!(Instant::now() < deadline, "{method} never returned");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Send desktop notifications to the bus at `address`.
    pub fn attach_desktop(&mut self, address: &str) {
        let window = self.window;
        let (notifier, clicks) = rexia::notify::Notifier::spawn(Some(address.to_owned()));
        self.cx.update(|cx| {
            window
                .update(cx, |ws, window, cx| {
                    ws.set_notifier(notifier, clicks, window, cx)
                })
                .unwrap()
        });
        self.pump();
    }

    /// Give the window the desktop's focus, as when the user is looking at it.
    pub fn activate(&mut self) {
        let window = self.window;
        self.cx.update(|cx| {
            window
                .update(cx, |_, window, _| window.activate_window())
                .unwrap()
        });
        self.pump();
    }

    pub fn workspace<R>(&mut self, read: impl FnOnce(&Workspace) -> R) -> R {
        let window = self.window;
        self.cx.update(|cx| read(window.read(cx).unwrap()))
    }

    /// Run a rexia action as the palette would.
    pub fn run(&mut self, action: &str, args: Value) {
        let window = self.window;
        self.cx.update(|cx| {
            window
                .update(cx, |ws, window, cx| {
                    ws.run_action(action, args, "palette", Default::default(), window, cx)
                        .unwrap();
                })
                .unwrap()
        });
        self.pump();
    }

    pub fn screenshot(&mut self, name: &str) -> PathBuf {
        std::thread::sleep(Duration::from_millis(250));
        self.pump();
        let image = self.cx.capture_screenshot(self.window.into()).unwrap();
        let path = shot_dir().join(format!("{name}.png"));
        image.save(&path).unwrap();
        println!("saved {}", path.display());
        path
    }
}
