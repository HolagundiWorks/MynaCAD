#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

// ---------------------------------------------------------------- fakes

/// Kivo: answers by `op`. `ask` is called for every ask_session and decides the answer.
struct FakeKivo {
    asks: AtomicUsize,
    ask: Box<dyn Fn(usize) -> Value + Send + Sync>,
    reachable: bool,
}

impl Transport for FakeKivo {
    fn request(&self, message: &str) -> std::io::Result<String> {
        if !self.reachable {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no pipe"));
        }
        let v: Value = serde_json::from_str(message).unwrap();
        let reply = match v["op"].as_str().unwrap() {
            "status" => json!({ "v": 1, "ok": true, "signed_in": true }),
            "ask_session" => {
                assert_eq!(v["app"], "tren");
                assert_eq!(v["purpose"], "db_connect");
                let n = self.asks.fetch_add(1, Ordering::SeqCst);
                (self.ask)(n)
            }
            other => panic!("unexpected op {other}"),
        };
        Ok(reply.to_string())
    }
}

fn token_reply(n: usize) -> Value {
    json!({ "v": 1, "ok": true, "token": format!("token-{n}"), "expires_utc": "2026-11-09T00:00:00Z", "user": "owner" })
}

fn kivo(ask: impl Fn(usize) -> Value + Send + Sync + 'static) -> Arc<FakeKivo> {
    Arc::new(FakeKivo { asks: AtomicUsize::new(0), ask: Box::new(ask), reachable: true })
}

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: Value,
}

/// Myna: a loopback HTTP server that records what it is sent and answers with `respond`.
struct FakeMyna {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn myna(respond: impl Fn(&Seen, usize) -> (u16, Value) + Send + Sync + 'static) -> FakeMyna {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
            let (mut auth, mut length) = (String::new(), 0usize);
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end().to_string();
                if h.is_empty() {
                    break;
                }
                let lower = h.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if lower.starts_with("authorization:") {
                    auth = h["authorization:".len()..].trim().to_string();
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let request = Seen { method, path, auth, body: serde_json::from_slice(&body).unwrap_or(Value::Null) };
            let n = {
                let mut g = log.lock().unwrap();
                g.push(request.clone());
                g.len()
            };
            let (code, payload) = respond(&request, n);
            let text = payload.to_string();
            let reason = if code == 200 { "OK" } else { "Error" };
            let _ = write!(stream, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
        }
    });
    FakeMyna { url, seen }
}

fn cfg(kivo: Arc<FakeKivo>, url: &str) -> Config {
    Config {
        transport: kivo,
        service: Service::new(url),
        launch_kivo: Arc::new(|| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not installed"))),
        app: "tren",
        interactive: true,
        kivo_wait: Duration::from_millis(50),
        push_delay: Duration::from_millis(30),
    }
}

/// A real project file with a known id, name and revision.
fn project(dir: &tempfile::TempDir) -> (String, String) {
    let path = dir.path().join("house.osp");
    let odp = aad_odp::Odp::create(&path, "House").unwrap();
    let meta = odp.meta().unwrap();
    (path.to_string_lossy().into_owned(), meta.project_id)
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

// ---------------------------------------------------------------- tests

#[test]
fn the_status_text_says_what_a_person_can_do() {
    let at = |state, pending| Snapshot { state, pending, ..Snapshot::default() }.label();
    assert_eq!(at(State::Idle, 0), "");
    assert_eq!(at(State::Connected, 0), "Database: connected");
    assert_eq!(at(State::Connected, 2), "Database: connected, 2 to send");
    assert_eq!(at(State::SignedOut, 0), "Database: sign in to Kivo");
    assert_eq!(at(State::Offline, 0), "Database: offline");
}

#[test]
fn opening_a_project_asks_kivo_then_tells_the_service_with_that_token() {
    let dir = tempfile::tempdir().unwrap();
    let (path, id) = project(&dir);
    let service = myna(|_, _| (200, json!({ "connected": true, "remote_revision": 1, "action": "none" })));
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);

    c.connect_blocking(&path);

    let snap = c.snapshot();
    assert_eq!(snap.state, State::Connected, "{snap:?}");
    assert_eq!(snap.project.as_deref(), Some("House"));
    let seen = service.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/v1/projects/connect"));
    assert_eq!(seen[0].auth, "Bearer token-0");
    assert_eq!(seen[0].body["project_id"], id);
    assert_eq!(seen[0].body["osp_path"], path);
    assert_eq!(seen[0].body["name"], "House");
}

#[test]
fn nobody_signed_in_is_reported_and_the_service_is_not_called() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|_, _| (200, json!({})));
    let c = Connector::new(cfg(kivo(|_| json!({ "v": 1, "ok": false, "error": "signed_out", "message": "Kivo is not signed in." })), &service.url), true);

    c.connect_blocking(&path);

    assert_eq!(c.snapshot().state, State::SignedOut);
    assert_eq!(c.snapshot().label(), "Database: sign in to Kivo");
    assert!(service.seen.lock().unwrap().is_empty());
}

#[test]
fn a_closed_sign_in_is_the_same_as_signed_out() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|_, _| (200, json!({})));
    let c = Connector::new(cfg(kivo(|_| json!({ "v": 1, "ok": false, "error": "cancelled", "message": "closed" })), &service.url), true);
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::SignedOut);
}

#[test]
fn a_role_without_access_is_a_problem_not_a_retry() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|_, _| (200, json!({})));
    let k = kivo(|_| json!({ "v": 1, "ok": false, "error": "denied", "message": "Your role cannot do this." }));
    let c = Connector::new(cfg(Arc::clone(&k), &service.url), true);
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::Error);
    assert_eq!(k.asks.load(Ordering::SeqCst), 1);
}

#[test]
fn no_kivo_means_offline_and_the_drawing_is_not_touched() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let k = Arc::new(FakeKivo { asks: AtomicUsize::new(0), ask: Box::new(token_reply), reachable: false });
    let c = Connector::new(cfg(k, "http://127.0.0.1:9"), true);
    let before = std::fs::read(&path).unwrap();
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::Offline);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn no_myna_means_offline_with_a_hint() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    // Nothing listens on this port.
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", free.local_addr().unwrap());
    drop(free);
    let c = Connector::new(cfg(kivo(token_reply), &url), true);
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::Offline);
    assert!(c.snapshot().detail.contains("Start Myna"), "{}", c.snapshot().detail);
}

#[test]
fn a_refused_token_is_replaced_once_by_asking_kivo_again() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|req, _| {
        if req.auth == "Bearer token-0" {
            (401, json!({ "ok": false, "error": "bad_token" }))
        } else {
            (200, json!({ "connected": true, "remote_revision": null, "action": "none" }))
        }
    });
    let k = kivo(token_reply);
    let c = Connector::new(cfg(Arc::clone(&k), &service.url), true);
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::Connected, "{:?}", c.snapshot());
    assert_eq!(k.asks.load(Ordering::SeqCst), 2);
    let seen = service.seen.lock().unwrap();
    assert_eq!(seen.last().unwrap().auth, "Bearer token-1");
}

#[test]
fn a_newer_shared_copy_is_reported_and_never_applied() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|_, _| (200, json!({ "connected": true, "remote_revision": 9, "action": "pull" })));
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    let before = std::fs::read(&path).unwrap();
    c.connect_blocking(&path);
    assert_eq!(c.snapshot().state, State::Connected);
    assert!(c.snapshot().detail.contains("newer revision"), "{}", c.snapshot().detail);
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file must not change");
    // Only the connect call: no pull was asked for.
    assert_eq!(service.seen.lock().unwrap().len(), 1);
}

#[test]
fn a_service_that_wants_a_push_gets_the_current_revision() {
    let dir = tempfile::tempdir().unwrap();
    let (path, id) = project(&dir);
    let service = myna(|req, _| match req.path.as_str() {
        "/v1/projects/connect" => (200, json!({ "connected": true, "remote_revision": 0, "action": "push" })),
        p if p.ends_with("/push") => (200, json!({ "remote_revision": 1 })),
        _ => (200, json!({ "local_revision": 1, "remote_revision": 1, "state": "synced", "pending": 0 })),
    });
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    c.connect_blocking(&path);
    let seen = service.seen.lock().unwrap();
    let push = seen.iter().find(|s| s.path.ends_with("/push")).expect("a push was sent");
    assert_eq!(push.path, format!("/v1/projects/{id}/push"));
    assert_eq!(push.auth, "Bearer token-0");
    assert!(push.body["revision"].is_number());
    assert_eq!(c.snapshot().state, State::Connected);
}

#[test]
fn a_conflict_on_push_keeps_both_copies_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|req, _| match req.path.as_str() {
        "/v1/projects/connect" => (200, json!({ "connected": true, "remote_revision": 1, "action": "none" })),
        _ => (409, json!({ "ok": false, "error": "conflict" })),
    });
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    c.connect_blocking(&path);
    c.push_now(3);
    let snap = c.snapshot();
    assert_eq!(snap.state, State::Connected);
    assert!(snap.detail.contains("newer revision") && snap.detail.contains("Nothing was overwritten"), "{}", snap.detail);
}

#[test]
fn a_service_that_keeps_files_local_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|req, _| match req.path.as_str() {
        "/v1/projects/connect" => (200, json!({ "connected": true, "remote_revision": null, "action": "none" })),
        p if p.ends_with("/push") => (200, json!({ "remote_revision": null, "local_only": true })),
        _ => (200, json!({ "local_revision": 1, "remote_revision": null, "state": "synced", "pending": 0 })),
    });
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    c.connect_blocking(&path);
    c.push_now(4);
    let snap = c.snapshot();
    assert_eq!(snap.state, State::Connected);
    assert!(snap.detail.contains("stay on this PC"), "{}", snap.detail);
    assert_eq!(snap.label(), "Database: connected");
}

#[test]
fn several_saves_in_a_row_go_as_one_push() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|req, _| match req.path.as_str() {
        "/v1/projects/connect" => (200, json!({ "connected": true, "remote_revision": 1, "action": "none" })),
        p if p.ends_with("/push") => (200, json!({ "remote_revision": 2 })),
        _ => (200, json!({ "local_revision": 2, "remote_revision": 2, "state": "synced", "pending": 0 })),
    });
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    c.connect_blocking(&path);
    for _ in 0..3 {
        c.project_saved(&path);
    }
    wait_for("the push", || service.seen.lock().unwrap().iter().any(|s| s.path.ends_with("/push")));
    std::thread::sleep(Duration::from_millis(200));
    let pushes = service.seen.lock().unwrap().iter().filter(|s| s.path.ends_with("/push")).count();
    assert_eq!(pushes, 1);
}

#[test]
fn a_disabled_connector_never_talks_to_anyone() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = project(&dir);
    let service = myna(|_, _| (200, json!({})));
    let k = kivo(token_reply);
    let c = Connector::new(cfg(Arc::clone(&k), &service.url), false);
    c.project_opened(&path);
    c.project_saved(&path);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(k.asks.load(Ordering::SeqCst), 0);
    assert!(service.seen.lock().unwrap().is_empty());
    assert_eq!(c.snapshot().state, State::Idle);
}

#[test]
fn a_file_that_is_not_a_project_is_ignored() {
    let service = myna(|_, _| (200, json!({})));
    let c = Connector::new(cfg(kivo(token_reply), &service.url), true);
    c.connect_blocking("C:/drawings/plan.dxf");
    assert_eq!(c.snapshot().state, State::Idle);
}

#[test]
fn the_session_token_never_shows_in_debug_output() {
    let s = broker::Session { token: "secret-token".into(), expires_utc: "x".into(), user: "owner".into() };
    assert!(!format!("{s:?}").contains("secret-token"));
}

/// Against the installed Kivo and Myna: `cargo test -p aad-dbconnect -- --ignored --nocapture`. Starts Kivo hidden if it is not running.
#[test]
#[ignore = "needs the installed suite"]
fn live_kivo_pipe_and_myna_service() {
    let t = broker::PipeTransport;
    broker::ensure_broker(&t, &broker::launch_kivo, Duration::from_secs(30)).expect("Kivo answers on its pipe");
    let signed_in = broker::status(&t).expect("status");
    println!("Kivo signed in: {signed_in}");
    let ask = broker::ask_session(&t, "tren", "db_connect", None, false);
    println!("silent ask: {ask:?}");
    if !signed_in {
        assert_eq!(ask.unwrap_err(), BrokerError::SignedOut);
    }
    match Service::from_env().health() {
        Ok(h) => println!("Myna: {h:?}"),
        Err(e) => println!("Myna: {e}"),
    }
}
