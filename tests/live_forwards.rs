mod common;

use common::{Harness, Sessions};
use rexia::ui::workspace::ListKind;
use serde_json::Value;
use std::{
    cell::RefCell,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    rc::Rc,
    time::Duration,
};

#[test]
fn manual_and_clicked_forwards_prefer_the_target_port_and_fall_back_if_busy() {
    let Ok(server) = std::env::var("REXIA_TEST_REMOTE_FORWARD_SERVER") else {
        eprintln!("skipped: set REXIA_TEST_REMOTE_FORWARD_SERVER to a disposable HTTP Rex server");
        return;
    };
    check_forward(&server, false);
    check_forward(&server, true);
}

fn check_forward(server: &str, occupied: bool) {
    // The disposable Rex server runs on this machine. Use a separate loopback
    // address so the destination itself doesn't occupy the client's bind address.
    let service = TcpListener::bind("127.0.0.2:0").unwrap();
    let port = service.local_addr().unwrap().port();
    let _blocker = occupied.then(|| TcpListener::bind(("127.0.0.1", port)).unwrap());
    let sessions = Sessions::create(&common::connect(server), "rexia-forwards");
    let mut h = Harness::open(server, &sessions.ids[0], 1000., 680.);
    h.wait("selected session", |m| m.view().map(|_| ()));
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |url, _| output.borrow_mut().push(url.into()))
            })
            .unwrap()
    });
    h.press("ctrl-shift-p");
    h.type_text("Forward Port");
    h.press("enter");
    assert_eq!(h.workspace(|ws| ws.list_kind()), Some(ListKind::Forwards));
    h.type_text(&format!("127.0.0.2:{port}"));
    h.press("enter");
    let local = h.wait_ui("manual forward opens", |_| opened.borrow().first().cloned());
    let local = url::Url::parse(&local).unwrap();
    let local_port = local.port().unwrap();
    if occupied {
        assert_ne!(
            local_port, port,
            "choose a free port when the target port is busy"
        );
    } else {
        assert_eq!(local_port, port, "prefer the target port when it is free");
    }
    let mut client = TcpStream::connect(("127.0.0.1", local_port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let echo = std::thread::spawn(move || {
        let (mut connection, _) = service.accept().unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut marker = [0; 4];
        connection.read_exact(&mut marker).unwrap();
        connection.write_all(&marker).unwrap();
    });
    client.write_all(b"test").unwrap();
    let mut echoed = [0; 4];
    client.read_exact(&mut echoed).unwrap();
    assert_eq!(&echoed, b"test");
    echo.join().unwrap();

    let link = format!("http://127.0.0.2:{port}/app?q=1#section");
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.open_link(link, cx))
            .unwrap()
    });
    let clicked = h.wait_ui("rewritten link opens", |_| opened.borrow().get(1).cloned());
    let clicked = url::Url::parse(&clicked).unwrap();
    assert_eq!(clicked.port(), Some(local_port));
    assert_eq!(clicked.path(), "/app");
    assert_eq!(clicked.query(), Some("q=1"));
    assert_eq!(clicked.fragment(), Some("section"));
    h.run("client.host.forward_port", Value::Null);
    assert_eq!(h.workspace(|ws| ws.list_titles().len()), 1);
    h.press("alt-enter");
    assert!(h.workspace(|ws| ws.list_titles().is_empty()));
    h.wait_ui("forward listener closes", |_| {
        TcpStream::connect(("127.0.0.1", local_port))
            .is_err()
            .then_some(())
    });
}
