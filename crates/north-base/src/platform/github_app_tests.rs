use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

const NOW: i64 = 1_800_000_000;

struct Seen {
    body: String,
}

struct Server {
    api: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    refuse: Arc<AtomicBool>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let refuse = Arc::new(AtomicBool::new(false));
        let (log, refusing) = (Arc::clone(&seen), Arc::clone(&refuse));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(": ").unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let mut log = log.lock().unwrap();
                log.push(Seen { body: String::from_utf8(body).unwrap() });
                let reply = if refusing.load(Ordering::SeqCst) {
                    ("401 Unauthorized", json!({ "message": "Bad   credentials" }))
                } else {
                    ("201 Created", json!({ "token": format!("ghs_test_{}", log.len()), "expires_at": Timestamp::from_unix_seconds(NOW + 3600).iso() }))
                };
                let body = reply.1.to_string();
                write!(stream, "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", reply.0, body.len()).unwrap();
            }
        });
        Self { api, seen, refuse }
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

struct Fixture {
    scratch: tempfile::TempDir,
    pem: String,
}

impl Fixture {
    fn new(app: &str) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let key = scratch.path().join("key");
        let made = Command::new("ssh-keygen").args(["-q", "-t", "rsa", "-b", "2048", "-m", "PEM", "-N", "", "-f"]).arg(&key).status().unwrap();
        assert!(made.success());
        let pem = fs::read_to_string(&key).unwrap();
        let file = scratch.path().join("secrets").join(app);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "").unwrap();
        fs::create_dir_all(scratch.path().join("run")).unwrap();
        Self { scratch, pem }
    }

    fn host(&self, server: &Server, creds: Value) -> Host {
        let decrypted = creds.to_string().into_bytes();
        Host { secrets: self.scratch.path().join("secrets"), runtime: Some(self.run()), pushed: None, api: server.api.clone(), decrypt: Box::new(move |_| Some(decrypted.clone())) }
    }

    fn run(&self) -> PathBuf {
        self.scratch.path().join("run")
    }
}

fn request(words: &[&str]) -> Request {
    match parse(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).unwrap() {
        Parsed::Mint(request) => request,
        Parsed::Help => panic!("help"),
    }
}

#[test]
fn a_refused_exchange_fails_without_the_key_or_a_cache() {
    let server = Server::start();
    server.refuse.store(true, Ordering::SeqCst);
    let fixture = Fixture::new("github-app.yaml");
    let host = fixture.host(&server, json!({ "app_id": 123, "installation_id": 42, "private_key": fixture.pem }));

    let error = mint(&request(&[]), &host, NOW).unwrap_err();
    assert_eq!(error, "token exchange failed: HTTP 401 Bad credentials");
    let key_line = fixture.pem.lines().nth(1).unwrap();
    assert!(!error.contains("PRIVATE KEY") && !error.contains(key_line));
    assert!(!fixture.run().join("gh-app-token").exists());
}

const WRITE_TOKEN_REPOS: &str = "north,south,firn,wisp,smashcraft,nixos-config";

#[test]
fn permissions_scope_the_six_repositories_and_never_grant_workflows() {
    let server = Server::start();
    let fixture = Fixture::new("github-app.yaml");
    let creds = json!({ "app_id": 123, "installation_id": 42, "private_key": fixture.pem });
    let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();

    let scoped = words(&["--repos", WRITE_TOKEN_REPOS, "--permissions", r#"{"pull_requests":"write","issues":"write"}"#]);
    assert_eq!(run_on(&scoped, || fixture.host(&server, creds.clone()), NOW), 0);
    let body: Value = serde_json::from_str(&server.seen.lock().unwrap()[0].body).unwrap();
    assert_eq!(
        body,
        json!({
            "repositories": ["north", "south", "firn", "wisp", "smashcraft", "nixos-config"],
            "permissions": { "contents": "write", "issues": "write", "metadata": "read", "pull_requests": "write" }
        })
    );
    assert!(body["permissions"].get("workflows").is_none());
    assert!(!fixture.run().join("gh-app-token").exists());

    for permissions in [r#"{"workflows":"write"}"#, r#"{"contents":"write","workflows":"read"}"#] {
        let refused = words(&["--repos", WRITE_TOKEN_REPOS, "--permissions", permissions]);
        assert_eq!(run_on(&refused, || fixture.host(&server, creds.clone()), NOW), 1);
        assert_eq!(parse(&refused), Err("--permissions may not grant workflows".to_owned()));
    }
    assert_eq!(server.requests(), 1);
}

#[test]
fn a_host_the_laptop_pushes_to_serves_the_pushed_token_while_fresh_and_never_decrypts() {
    let server = Server::start();
    let fixture = Fixture::new("github-app.yaml");
    let pushed = fixture.scratch.path().join("gh-write-token");
    fs::write(&pushed, "ghs_pushed\n").unwrap();
    let written = fs::metadata(&pushed).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let written = i64::try_from(written).unwrap();
    let host = Host { pushed: Some(pushed), decrypt: Box::new(|_| panic!("decrypted the App key on a host the laptop pushes to")), ..fixture.host(&server, json!({})) };

    assert_eq!(mint(&request(&[]), &host, written + 60), Ok("ghs_pushed".to_owned()));
    let stale = mint(&request(&[]), &host, written + 55 * 60).unwrap_err();
    assert!(stale.contains("55 min old"), "{stale}");
    for words in [&["--app", "lander"][..], &["--repos", "north"][..]] {
        assert!(mint(&request(words), &host, written + 60).unwrap_err().contains("holds no App key"));
    }
    assert_eq!(server.requests(), 0);
}
