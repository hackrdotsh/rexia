//! Keys, modes, init.lua bindings and remote control against a real Rex server. Set
//! REXIA_TEST_SERVER to a disposable server whose config is tests/fixtures/init.lua
//! (`rex server run --config tests/fixtures/init.lua`).
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
    sync::Arc,
    time::{Duration, Instant},
};

struct Harness {
    cx: HeadlessAppContext,
    window: WindowHandle<Workspace>,
    host: HostHandle,
}

impl Harness {
    fn pump(&mut self) {
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

    fn wait<T>(&mut self, what: &str, mut check: impl FnMut(&HostModel) -> Option<T>) -> T {
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

    fn press(&mut self, keys: &str) {
        for key in keys.split(' ') {
            let key: &'static str = Box::leak(key.to_owned().into_boxed_str());
            let mut keystroke = Keystroke::parse(key).unwrap();
            if keystroke.key.chars().count() == 1 && !keystroke.modifiers.control {
                keystroke.key_char = Some(keystroke.key.clone());
            }
            let handle: AnyWindowHandle = self.window.into();
            self.cx
                .update_window(handle, |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                })
                .unwrap();
            self.pump();
        }
    }

    /// Call a method from another client while rexia keeps running, as it would.
    fn call_from(
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

    fn workspace<R>(&mut self, read: impl FnOnce(&Workspace) -> R) -> R {
        let window = self.window;
        self.cx.update(|cx| read(window.read(cx).unwrap()))
    }

    fn windows(&mut self) -> usize {
        self.host.snapshot().view().map_or(0, |v| v.windows.len())
    }
}

fn request(host: &HostHandle, method: &str, payload: Value) -> Result<Value, String> {
    let (reply, answer) = tokio::sync::oneshot::channel();
    host.send(HostCommand::Call {
        method: method.into(),
        payload,
        reply: Some(reply),
    });
    answer.blocking_recv().unwrap()
}

#[test]
fn keys_modes_lua_bindings_and_remote_control() {
    let Ok(address) = std::env::var("REXIA_TEST_SERVER") else {
        eprintln!("skipped: set REXIA_TEST_SERVER to a disposable Rex server");
        return;
    };
    let control = HostHandle::spawn(
        Endpoint::parse(&address).unwrap(),
        Identity::default(),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while control.snapshot().connection != Connection::Connected {
        assert!(Instant::now() < deadline);
        let _ = control.wake.recv_timeout(Duration::from_millis(50));
    }
    let (reply, answer) = tokio::sync::oneshot::channel();
    control.send(HostCommand::CreateSession {
        label: Some("rexia-keys".into()),
        reply: Some(reply),
    });
    let session = answer.blocking_recv().unwrap().unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut settings = Settings::default();
        settings.hosts = vec![HostConfig {
            id: "test".into(),
            label: "test".into(),
            endpoint: address.clone(),
        }];
        settings.last_session = Some(session.clone());
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_wgpu::CosmicTextSystem::new("DM Sans")),
            Arc::new(Assets),
            || gpui_platform::current_headless_renderer(),
        );
        cx.update(|cx| cx.set_global(Theme::from_noctalia(&noctalia::load())));
        let window = cx
            .open_window(size(px(1200.), px(760.)), |window, cx| {
                cx.new(|cx| Workspace::new(settings, None, false, window, cx))
            })
            .unwrap();
        let host = cx.update(|cx| window.read(cx).unwrap().host().handle.clone());
        let mut h = Harness { cx, window, host };

        let keymap = h.wait("the keymap and a live pane", |m| {
            let pane = m.view()?.active_window()?.focused_block_id.clone()?;
            m.panes.get(&pane)?.snapshot().frame?;
            m.keymap.clone()
        });
        let bindings = keymap["bindings"].as_array().unwrap();
        println!(
            "keymap: {} bindings, {} from init.lua, diagnostics {}",
            bindings.len(),
            bindings
                .iter()
                .filter(|b| b["source"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("init.lua")))
                .count(),
            keymap["diagnostics"]
        );
        // Let the shell draw its prompt before typing into it.
        h.wait("a prompt", |m| (!h_text_empty(m)).then_some(()));

        // A rexia default: ctrl+shift+t opens a tab.
        let before = h.windows();
        h.press("ctrl-shift-t");
        h.wait("ctrl+shift+t to open a tab", |m| {
            (m.view()?.windows.len() == before + 1).then_some(())
        });
        println!(
            "ctrl+shift+t (rexia default): tabs {before} -> {}",
            before + 1
        );

        // An init.lua key sequence: ctrl+a then c also opens a tab, with a hint in between.
        h.press("ctrl-a");
        assert!(
            h.workspace(|ws| ws.pending_keys() == 1),
            "ctrl+a waits for the next key"
        );
        h.press("c");
        h.wait("ctrl+a c to open a tab", |m| {
            (m.view()?.windows.len() == before + 2).then_some(())
        });
        println!("ctrl+a c (init.lua sequence): tabs -> {}", before + 2);

        // init.lua rebinds ctrl+shift+o from Split Right to a downward split.
        h.press("ctrl-shift-o");
        let direction = h.wait("ctrl+shift+o to split", |m| {
            let window = m.view()?.active_window()?;
            (window.panes().len() == 2).then(|| window.dividers()[0].horizontal)
        });
        assert!(
            !direction,
            "init.lua's downward split wins over rexia's default"
        );
        println!("ctrl+shift+o (init.lua override): split down");

        // An exclusive mode: keys it doesn't bind are swallowed; its own binding runs.
        h.press("ctrl-alt-r");
        assert_eq!(
            h.workspace(|ws| ws.active_mode()),
            Some("resize".to_owned())
        );
        h.press("enter");
        assert_eq!(h.workspace(|ws| ws.active_mode()), None);
        println!("ctrl+alt+r enters resize mode; enter (bound in the mode) leaves it");

        // A key bound to a Lua function goes to the server, which writes into the pane.
        h.press("ctrl-shift-u");
        h.wait("the Lua binding's output", |m| {
            h_text(m).contains("rexia-lua-key").then_some(())
        });
        println!("ctrl+shift+u (Lua function): wrote rexia-lua-key");

        // Remote control: another client asks rexia to zoom, then asks for something it lacks.
        let client_id = h.host.snapshot().client_id.unwrap();
        let zoom = h.call_from(
            &control,
            "client.perform",
            json!({"client_id": client_id, "action": "pane.zoom", "session_id": session}),
        );
        assert!(zoom.is_ok(), "pane.zoom: {zoom:?}");
        h.wait("the zoom", |m| {
            m.view()?.active_window()?.zoomed_block_id.clone()
        });
        let missing = h.call_from(
            &control,
            "client.perform",
            json!({"client_id": client_id, "action": "client.quick_look"}),
        );
        println!("client.perform pane.zoom -> ok; client.quick_look -> {missing:?}");
        assert!(missing.is_err());

        // rex do: the CLI drives rexia by its client label.
        let inspect = h
            .call_from(&control, "client.inspect", json!({"client_id": client_id}))
            .unwrap();
        let declared = inspect["actions"].as_array().map_or(0, Vec::len);
        println!("client.inspect lists {declared} rexia actions");
        assert!(declared > 50);
    }));
    let _ = request(&control, "session.destroy", json!({"session_id": session}));
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn h_text(model: &HostModel) -> String {
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

fn h_text_empty(model: &HostModel) -> bool {
    h_text(model).trim().is_empty()
}
