//! Tailscale notifications, against a real Rex server named by REXIA_TEST_SERVER and a
//! private session bus. Built-in Tailscale's state changes as `tailscale_status_changed`
//! events would change it in rexia's host model: needing a sign-in notifies (and a click
//! opens the sign-in link), needing an admin's approval notifies and takes the sign-in
//! notice back, running takes the approval notice back, stopping with an error notifies,
//! and each switch under Settings › Notifications turns its notices off.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

fn tailscale(h: &mut Harness, status: Value) {
    {
        let mut model = h.host.model.lock().unwrap();
        model.server["tailscale"] = status;
        model.tailscale_revision += 1;
    }
    h.pump();
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
}

#[test]
fn tailscale_changes_notify_and_clear() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-tailscale-notes");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.attach_desktop(&desktop.address);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let record = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()))
            })
            .unwrap()
    });
    tailscale(&mut h, json!({"state": "disabled"}));
    let start = desktop.shown().len();

    // Needs a sign-in: a notice, and clicking it opens the link.
    tailscale(
        &mut h,
        json!({"state": "needs_login", "auth_url": "https://login.tailscale.com/a/abc123"}),
    );
    let sign_in = h.wait("the sign-in notice", |_| {
        desktop.shown().get(start).cloned()
    });
    println!("sign-in: {:?} / {:?}", sign_in.summary, sign_in.body);
    assert_eq!(sign_in.summary, "Tailscale needs you to sign in");
    desktop.click(sign_in.id);
    h.wait("the sign-in link opened", |_| {
        opened
            .borrow()
            .last()
            .cloned()
            .filter(|l| l.contains("abc123"))
    });
    println!("clicked: opened {:?}", opened.borrow());

    // Needs approval: a notice, and the sign-in notice goes.
    tailscale(&mut h, json!({"state": "needs_machine_approval"}));
    let approval = h.wait("the approval notice", |_| {
        desktop
            .shown()
            .into_iter()
            .find(|s| s.summary.contains("approval"))
    });
    h.wait("the sign-in notice closed", |_| {
        desktop.closed().contains(&sign_in.id).then_some(())
    });
    println!("approval: {:?}; sign-in notice closed", approval.summary);

    // Running: the approval notice goes.
    tailscale(
        &mut h,
        json!({"state": "running", "dns_name": "flareon.ts.net"}),
    );
    h.wait("the approval notice closed", |_| {
        desktop.closed().contains(&approval.id).then_some(())
    });
    println!("running: approval notice closed");

    // Stopping with an error: a critical notice.
    tailscale(
        &mut h,
        json!({"state": "disabled", "error": "control server unreachable"}),
    );
    let stopped = h.wait("the failure notice", |_| {
        desktop
            .shown()
            .into_iter()
            .find(|s| s.summary == "Tailscale stopped")
    });
    println!("stopped: {:?} (urgency {})", stopped.body, stopped.urgency);
    assert!(stopped.body.contains("control server unreachable"));
    assert_eq!(stopped.urgency, 2);

    // With failures off, a new error says nothing.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.notifications.tailscale_failures = false, cx)
            })
            .unwrap()
    });
    let before = desktop.shown().len();
    tailscale(
        &mut h,
        json!({"state": "disabled", "error": "a different problem"}),
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    h.pump();
    println!(
        "failures off: {} new notices",
        desktop.shown().len() - before
    );
    assert_eq!(desktop.shown().len(), before);
    drop(sessions);
}
