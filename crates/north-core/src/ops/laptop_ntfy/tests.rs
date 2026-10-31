use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::http::{Log, Respond, serve};
use super::{Config, Ctx};

struct Rig {
    root: tempfile::TempDir,
    config: Arc<Config>,
    now: Arc<Mutex<String>>,
    published: Arc<Mutex<Vec<(String, Value)>>>,
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

fn reply_all(listener: TcpListener, published: Arc<Mutex<Vec<(String, Value)>>>) {
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut data = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                data.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&data).into_owned();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = head.lines().find_map(|line| line.strip_prefix("Content-Length: ")?.parse().ok()).unwrap();
                    if body.len() >= length {
                        let auth = head.lines().find_map(|line| line.strip_prefix("Authorization: ")).unwrap_or_default().to_owned();
                        published.lock().unwrap().push((auth, serde_json::from_str(body).unwrap()));
                        break;
                    }
                }
            }
            stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 11\r\n\r\n{\"id\":\"x\"}\n").unwrap();
        }
    });
}

impl Rig {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let stub = base.join("threads.sh");
        crate::foundation::shim::write_script(&stub, "echo '[]'\n");
        fs::write(base.join("window.json"), json!({"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
        fs::write(base.join("publisher"), "fixture-publisher\n").unwrap();
        fs::write(base.join("action"), "fixture-action\n").unwrap();
        let published = Arc::new(Mutex::new(Vec::new()));
        let ntfy = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", ntfy.local_addr().unwrap().port());
        reply_all(ntfy, Arc::clone(&published));
        let now = Arc::new(Mutex::new("2026-10-13T08:00:00Z".to_owned()));
        let (clock, path, script) = (Arc::clone(&now), base.to_owned(), stub.display().to_string());
        let config = Arc::new(Config {
            url,
            topic: "socrates-laptop".into(),
            token_file: base.join("publisher"),
            action_token_file: base.join("action"),
            action_url: "http://10.77.0.1:2588".into(),
            listen: String::new(),
            state_dir: base.join("ntfy-state"),
            window: base.join("window.json"),
            laptop: Arc::new(move || Ctx::fixture(&path, &clock.lock().unwrap(), &script)),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (handler, sink) = (Arc::clone(&config), Arc::clone(&log));
        let respond: Respond = Arc::new(move |request| handler.respond(request));
        let record: Log = Arc::new(move |line| sink.lock().unwrap().push(line.to_owned()));
        std::thread::spawn(move || serve(&listener, &respond, &record));
        Self { root, config, now, published, port, log }
    }

    fn at(&self, now: &str) {
        *self.now.lock().unwrap() = now.to_owned();
    }

    fn laptop(&self, args: &[&str]) {
        let arguments: Vec<String> = args.iter().map(|word| (*word).to_owned()).collect();
        (self.config.laptop)().main(&arguments).unwrap();
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.root.path().join("placement/laptop.json")).unwrap()).unwrap()
    }

    fn http(&self, method: &str, path: &str, token: &str) -> u16 {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n").unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    fn press(&self, verb: &str, token: &str) -> u16 {
        self.http("POST", &format!("/laptop/{verb}"), token)
    }

    fn notify(&self, now: &str) {
        self.at(now);
        let at = crate::foundation::clock::Timestamp::parse_rfc3339(now).unwrap().unix_seconds();
        self.config.notify(at).unwrap();
    }

    fn sent(&self) -> usize {
        self.published.lock().unwrap().len()
    }
}

#[test]
fn buttons_flip_the_state_attributed_to_ntfy_and_publish_nothing() {
    let rig = Rig::new();
    rig.laptop(&["open"]);
    rig.notify("2026-10-13T08:00:30Z");
    assert_eq!(rig.sent(), 0);
    assert_eq!(rig.press("close", "fixture-action"), 200);
    assert_eq!((rig.state()["state"].clone(), rig.state()["by"].clone()), (json!("draining"), json!("ntfy")));
    assert_eq!(rig.sent(), 0);
    assert_eq!(rig.press("open", "fixture-action"), 200);
    assert_eq!((rig.state()["state"].clone(), rig.state()["by"].clone()), (json!("available"), json!("ntfy")));
}

#[test]
fn a_bad_token_unknown_action_and_get_are_refused_and_change_nothing() {
    let rig = Rig::new();
    rig.laptop(&["open"]);
    assert_eq!(rig.press("close", "wrong"), 401);
    assert_eq!(rig.press("toggle", "fixture-action"), 404);
    assert_eq!(rig.http("GET", "/laptop/close", "fixture-action"), 405);
    assert_eq!(rig.state()["state"], "available");
    assert_eq!(rig.sent(), 0);
}

#[test]
fn the_token_never_reaches_the_endpoint_log() {
    let rig = Rig::new();
    rig.laptop(&["open"]);
    rig.press("close", "fixture-action");
    rig.press("open", "wrong");
    let log = rig.log.lock().unwrap().join("\n");
    assert!(log.contains("POST /laptop/close 200") && log.contains("POST /laptop/open 401"), "{log}");
    assert!(!log.contains("fixture-action"), "{log}");
}

#[test]
fn the_window_open_edge_sends_one_reminder_a_day_with_buttons_and_the_publisher_token() {
    let rig = Rig::new();
    rig.at("2026-10-13T08:00:00Z");
    rig.laptop(&["close", "until", "10:00"]);
    rig.notify("2026-10-13T14:30:00Z");
    assert_eq!(rig.sent(), 0);
    rig.notify("2026-10-13T15:05:00Z");
    assert_eq!(rig.sent(), 1);
    let (auth, body) = rig.published.lock().unwrap()[0].clone();
    assert_eq!(auth, "Bearer fixture-publisher");
    assert_eq!((body["title"].clone(), body["topic"].clone()), (json!("Laptop window opens 23:00"), json!("socrates-laptop")));
    let labels: Vec<&str> = body["actions"].as_array().unwrap().iter().map(|action| action["label"].as_str().unwrap()).collect();
    let urls: Vec<&str> = body["actions"].as_array().unwrap().iter().map(|action| action["url"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Open", "Close"]);
    assert_eq!(urls, ["http://10.77.0.1:2588/laptop/open", "http://10.77.0.1:2588/laptop/close"]);
    assert!(body["actions"].as_array().unwrap().iter().all(|action| action["action"] == "http" && action["method"] == "POST" && action["headers"]["Authorization"] == "Bearer fixture-action"));
    rig.notify("2026-10-13T15:20:00Z");
    assert_eq!(rig.sent(), 1);
    rig.notify("2026-10-14T15:05:00Z");
    assert_eq!(rig.sent(), 2);
}

#[test]
fn a_missing_action_token_file_fails_the_press_with_a_server_error() {
    let rig = Rig::new();
    fs::remove_file(Path::new(rig.root.path()).join("action")).unwrap();
    assert_eq!(rig.press("close", "fixture-action"), 500);
}
