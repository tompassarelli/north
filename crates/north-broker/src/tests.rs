use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

use north_base::foundation::clock::Timestamp;
use north_base::platform::broker_client::Client;
use north_base::platform::broker_wire::Request;
use serde_json::{Value, json};

use crate::credentials::{GITHUB_USER_FILE, Source};
use crate::github::Minter;
use crate::policy::Policy;
use crate::proxy::providers;
use crate::server::{Broker, Peer};

const NOW: i64 = 1_800_000_000;
const REAL_TOKEN: &str = "dop_real_secret";
const USER_TOKEN: &str = "ghp_owner_secret";
const POLICY: &str = r#"
[[caller]]
uid = 1001
[caller.github.agent]
repositories = ["north", "south"]
permissions = { contents = "write", issues = "write" }
[caller.proxy]
digitalocean = ["GET /v2/droplets", "GET /v2/echo", "POST /v2/droplets/*/actions", "POST /v2/body"]
github-user = ["POST /user/repos"]
"#;

type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, String)>>>;

fn fake_upstream(reply: impl Fn(&str, &str) -> (u16, Value) + Send + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let target = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let (mut headers, mut length) = (Vec::new(), 0);
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
                headers.push((name.to_owned(), value.to_owned()));
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let auth = headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("authorization")).map(|(_, value)| value.clone()).unwrap_or_default();
            log.lock().unwrap().push((target.clone(), headers, body.clone()));
            let (status, answer) = reply(&target, &format!("{auth}\n{body}"));
            let text = answer.to_string();
            write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nGitHub-Authentication-Token-Expiration: 2027-01-02 03:04:05 UTC\r\nX-OAuth-Scopes: repo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
        }
    });
    (base, seen)
}

fn github_reply(target: &str, request: &str) -> (u16, Value) {
    assert!(target.ends_with("/access_tokens"), "{target}");
    let asked: Value = serde_json::from_str(request.split_once('\n').unwrap().1).unwrap_or(Value::Null);
    let repositories: Vec<Value> = asked["repositories"].as_array().into_iter().flatten().map(|name| json!({ "name": name })).collect();
    (201, json!({ "token": "ghs_minted", "expires_at": Timestamp::from_unix_seconds(NOW + 3600).iso(), "permissions": asked["permissions"], "repositories": repositories }))
}

fn credentials() -> (tempfile::TempDir, Source) {
    let directory = tempfile::tempdir().unwrap();
    let key = directory.path().join("key");
    assert!(Command::new("ssh-keygen").args(["-q", "-t", "rsa", "-b", "2048", "-m", "PEM", "-N", "", "-f"]).arg(&key).status().unwrap().success());
    let pem = std::fs::read_to_string(&key).unwrap();
    std::fs::write(directory.path().join("github-app.yaml"), json!({ "app_id": 123, "installation_id": 42, "private_key": pem }).to_string()).unwrap();
    std::fs::write(directory.path().join("digitalocean.yaml"), json!({ "token": REAL_TOKEN }).to_string()).unwrap();
    std::fs::write(directory.path().join(GITHUB_USER_FILE), json!({ "token": USER_TOKEN }).to_string()).unwrap();
    let source = Source::Directory(directory.path().to_path_buf());
    (directory, source)
}

fn broker(github: &str, digitalocean: &str, uid: u32) -> (tempfile::TempDir, Broker) {
    let (directory, source) = credentials();
    let mut minter = Minter::new(source);
    minter.api = github.to_owned();
    let mut cloud = providers();
    cloud.iter_mut().for_each(|provider| digitalocean.clone_into(&mut provider.upstream));
    (directory, Broker::with(Policy::parse(&POLICY.replace("1001", &uid.to_string())).unwrap(), minter, cloud))
}

fn call(broker: &Broker, uid: u32, method: &str, target: &str, headers: &[(&str, &str)], body: &Value) -> (u16, Value) {
    let request = Request { method: method.to_owned(), target: target.to_owned(), headers: headers.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect(), body: body.to_string().into_bytes() };
    let answer = broker.handle(Peer { uid, pid: 1, lane_only: false }, &request, NOW);
    (answer.status, serde_json::from_slice(&answer.body).unwrap_or(Value::Null))
}

#[test]
fn a_call_outside_the_allowlist_is_refused_and_the_placeholder_never_becomes_the_token() {
    let (upstream, seen) = fake_upstream(|_, request| (200, json!({ "droplets": [], "echo": request })));
    let (_keep, broker) = broker("http://127.0.0.1:9", &upstream, 1001);
    let placeholder = [("Authorization", "Bearer placeholder")];

    let (status, answer) = call(&broker, 1001, "DELETE", "/digitalocean/v2/droplets/42", &placeholder, &Value::Null);
    assert_eq!(status, 403);
    assert!(answer["error"].as_str().unwrap().contains("DELETE /v2/droplets/42"), "{answer}");
    assert!(seen.lock().unwrap().is_empty(), "a refused call reached the provider");

    let (status, answer) = call(&broker, 1001, "GET", "/digitalocean/v2/echo", &placeholder, &Value::Null);
    assert_eq!(status, 200);
    let sent = seen.lock().unwrap();
    let authorization = sent[0].1.iter().find(|(name, _)| name.eq_ignore_ascii_case("authorization")).unwrap();
    assert_eq!(authorization.1, format!("Bearer {REAL_TOKEN}"));
    assert!(!answer.to_string().contains(REAL_TOKEN), "the real token came back: {answer}");
}

#[test]
fn a_scoped_token_is_minted_with_its_scope_and_inspected_back_through_the_socket() {
    let (github, seen) = fake_upstream(github_reply);
    let (keep, broker) = broker(&github, "http://127.0.0.1:9", rustix::process::getuid().as_raw());
    let socket = keep.path().join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let served = Arc::new(broker);
    std::thread::spawn(move || served.serve(&listener));
    let client = Client::at(&socket);

    let permissions = BTreeMap::from([("contents".to_owned(), "read".to_owned())]);
    let grant = client.github_token("agent", &["north".to_owned()], &permissions, 0).unwrap();
    assert_eq!(grant.repositories, Some(vec!["north".to_owned()]));
    assert_eq!(grant.permissions, BTreeMap::from([("contents".to_owned(), "read".to_owned()), ("metadata".to_owned(), "read".to_owned())]));
    let sent: Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
    assert_eq!(sent, json!({ "repositories": ["north"], "permissions": { "contents": "read", "metadata": "read" } }));
    let inspected = client.inspect(&grant.token).unwrap();
    assert_eq!(inspected["repositories"], json!(["north"]));
    assert_eq!(inspected["permissions"], json!({ "contents": "read", "metadata": "read" }));
    assert!(client.inspect("ghs_never_granted").is_err());
}

#[test]
fn a_caller_outside_the_ceiling_or_the_policy_gets_no_token_and_costs_no_mint() {
    let (github, seen) = fake_upstream(github_reply);
    let (_keep, broker) = broker(&github, "http://127.0.0.1:9", 1001);
    let ask = |uid, body: Value| call(&broker, uid, "POST", "/github/token", &[], &body);

    let (status, answer) = ask(1001, json!({ "app": "agent", "repositories": ["north"], "permissions": { "administration": "write" } }));
    assert_eq!((status, answer["error"].as_str().unwrap().contains("administration")), (403, true));
    assert_eq!(ask(1001, json!({ "app": "lander", "repositories": ["north"] })).0, 403, "the lander App goes only to a uid the policy names for it");
    assert_eq!(ask(4242, json!({ "app": "agent", "repositories": ["north"] })).0, 403);
    assert!(seen.lock().unwrap().is_empty(), "a refused request still minted a token");
    assert_eq!(ask(1001, json!({ "app": "agent", "repositories": ["north"], "permissions": { "contents": "read" } })).0, 200);
    let minted = || seen.lock().unwrap().len();
    let ask_at = |now: i64, valid_for: i64| {
        let body = json!({ "app": "agent", "repositories": ["north"], "permissions": { "contents": "read" }, "valid_for": valid_for });
        let request = Request { method: "POST".to_owned(), target: "/github/token".to_owned(), headers: Vec::new(), body: body.to_string().into_bytes() };
        broker.handle(Peer { uid: 1001, pid: 1, lane_only: false }, &request, now).status
    };
    assert_eq!((ask_at(NOW + 2000, 0), minted()), (200, 1), "a held token with 1600 s left serves a caller that asks nothing more");
    assert_eq!((ask_at(NOW + 2000, 1800), minted()), (200, 2), "a caller that stores the token for 1800 s gets a fresh one, not the held token about to expire");
    assert_eq!(ask_at(NOW + 2000, 3301), 400);
}

#[test]
fn a_glob_in_the_path_is_refused_and_a_brace_in_a_query_never_multiplies_a_call() {
    let (upstream, seen) = fake_upstream(|_, _| (200, json!({})));
    let (_keep, broker) = broker("http://127.0.0.1:9", &upstream, 1001);
    let headers = [("Authorization", "Bearer placeholder")];
    assert_eq!(call(&broker, 1001, "POST", "/digitalocean/v2/droplets/{..}/actions", &headers, &Value::Null).0, 403);
    assert_eq!(call(&broker, 1001, "POST", "/digitalocean/v2/droplets/[1-9]/actions", &headers, &Value::Null).0, 403);
    assert_eq!(call(&broker, 1001, "GET", "/digitalocean/v2/droplets?x={a,b}", &headers, &Value::Null).0, 403);
    assert!(seen.lock().unwrap().is_empty(), "a globbed call reached the provider");
}

#[test]
fn a_body_starting_with_at_reaches_the_provider_as_text_and_no_loaded_secret_comes_back() {
    let (upstream, seen) = fake_upstream(|_, request| (200, json!({ "echo": request })));
    let (keep, broker) = broker("http://127.0.0.1:9", &upstream, 1001);
    let marker = keep.path().join("github-app.yaml");
    let body = format!("@{}", marker.display());
    let request = Request { method: "POST".to_owned(), target: "/digitalocean/v2/body".to_owned(), headers: vec![("Authorization".to_owned(), "Bearer placeholder".to_owned())], body: body.clone().into_bytes() };
    let answer = broker.handle(Peer { uid: 1001, pid: 1, lane_only: false }, &request, NOW);
    assert_eq!(answer.status, 200);
    assert_eq!(seen.lock().unwrap()[0].2, body, "the provider got something other than the text the caller sent");
    assert!(!String::from_utf8_lossy(&answer.body).contains("BEGIN"), "a credential file came back");
}

#[test]
fn a_lane_only_session_gets_no_github_token_and_no_cloud_call() {
    let (upstream, _seen) = fake_upstream(|_, request| (200, json!({ "echo": request })));
    let (_keep, broker) = broker("http://127.0.0.1:9", &upstream, 1001);
    let ask = |target: &str, headers: &[(&str, &str)]| {
        let request = Request { method: "POST".to_owned(), target: target.to_owned(), headers: headers.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect(), body: json!({ "app": "agent" }).to_string().into_bytes() };
        broker.handle(Peer { uid: 1001, pid: 1, lane_only: true }, &request, NOW).status
    };
    assert_eq!(ask("/github/token", &[]), 403);
    assert_eq!(ask("/digitalocean/v2/customers/my/balance", &[("Authorization", "Bearer placeholder")]), 403);
}

#[test]
fn create_repository_is_always_private() {
    let (upstream, seen) = fake_upstream(|_, request| (201, json!({ "echo": request })));
    let (_keep, broker) = broker("http://127.0.0.1:9", &upstream, 1001);
    let headers = [("Authorization", "Bearer placeholder")];
    let create = |body: &Value| call(&broker, 1001, "POST", "/github-user/user/repos", &headers, body);

    let private = json!({ "name": "north-new", "private": true });
    assert_eq!(create(&private).0, 201);
    {
        let sent = seen.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "/user/repos");
        assert_eq!(sent[0].2, private.to_string());
        let authorization = sent[0].1.iter().find(|(name, _)| name.eq_ignore_ascii_case("authorization")).unwrap();
        assert_eq!(authorization.1, format!("Bearer {USER_TOKEN}"));
    }

    for refused in [
        json!({ "name": "north-new" }),
        json!({ "name": "north-new", "private": false }),
        json!({ "name": "north-new", "private": "true" }),
        json!({ "name": "north-new", "private": true, "visibility": "public" }),
        json!({ "name": "north-new", "private": true, "visibility": "internal" }),
        json!({ "name": "north-new", "private": true, "auto_init": true }),
        json!({ "name": "../north", "private": true }),
        json!({ "name": "a/b", "private": true }),
        json!({ "name": "..", "private": true }),
        json!({ "private": true }),
        json!(["north-new"]),
    ] {
        let (status, answer) = create(&refused);
        assert_eq!(status, 403, "{refused} was not refused: {answer}");
    }
    assert_eq!(seen.lock().unwrap().len(), 1, "a refused creation reached GitHub");

    for (method, target) in [("GET", "/github-user/user"), ("POST", "/github-user/user/repos?x=1"), ("DELETE", "/github-user/repos/tom/north"), ("POST", "/github-user/orgs/acme/repos")] {
        assert_eq!(call(&broker, 1001, method, target, &headers, &private).0, 403, "{method} {target} was not refused");
    }
    assert_eq!(seen.lock().unwrap().len(), 1, "a call other than POST /user/repos reached GitHub");
}

const PLANTED: &str = "github_pat_PLANTEDfakeOWNERtoken0123456789";

struct Copied {
    text: String,
    cleared: std::cell::Cell<bool>,
}

impl crate::intake::Clipboard for Copied {
    fn read(&self) -> Result<String, String> {
        Ok(format!("{}\n", self.text))
    }

    fn clear(&self) -> Result<(), String> {
        self.cleared.set(true);
        Ok(())
    }
}

fn south_with_a_recipient() -> (tempfile::TempDir, std::path::PathBuf) {
    let south = tempfile::tempdir().unwrap();
    let key = south.path().join("age.txt");
    assert!(Command::new("age-keygen").arg("-o").arg(&key).output().unwrap().status.success());
    let recipient = Command::new("age-keygen").arg("-y").arg(&key).output().unwrap().stdout;
    std::fs::create_dir_all(south.path().join("secrets")).unwrap();
    std::fs::create_dir_all(south.path().join("agents")).unwrap();
    std::fs::write(south.path().join(".sops.yaml"), format!("creation_rules:\n  - path_regex: secrets/[^/]+\\.yaml$\n    age: {}\n", String::from_utf8(recipient).unwrap().trim())).unwrap();
    std::fs::write(south.path().join("agents/credentials.toml"), "[[credential]]\nsource = \"secrets/github-user.yaml\"\nreach = \"broker:github-user\"\nuse = \"the owner's token\"\nkind = \"machine\"\nrenewal = \"tom: mint a new token\"\n").unwrap();
    (south, key)
}

#[test]
fn the_owner_token_is_stored_encrypted_and_never_printed() {
    let (south, key) = south_with_a_recipient();
    let secrets = south.path().join("secrets");
    let (github, seen) = fake_upstream(|_, _| (200, json!({ "login": "owner" })));
    let clipboard = Copied { text: PLANTED.to_owned(), cleared: std::cell::Cell::new(false) };
    let mut printed = Vec::new();

    crate::intake::run(&secrets, &github, &clipboard, &mut printed).unwrap();

    let printed = String::from_utf8(printed).unwrap();
    assert_eq!(printed, "expires: 2027-01-02 03:04:05 UTC\npermissions: repo\n");
    assert!(clipboard.cleared.get(), "the clipboard still holds the token");
    let sent = seen.lock().unwrap();
    assert_eq!(sent[0].0, "/user");
    assert_eq!(sent[0].1.iter().find(|(name, _)| name.eq_ignore_ascii_case("authorization")).unwrap().1, format!("Bearer {PLANTED}"));
    let stored = std::fs::read(secrets.join(GITHUB_USER_FILE)).unwrap();
    assert!(!stored.windows(PLANTED.len()).any(|window| window == PLANTED.as_bytes()), "the stored file holds the token in the clear");
    let decrypted = Command::new("sops").env("SOPS_AGE_KEY_FILE", &key).args(["-d", "--extract", "[\"token\"]"]).arg(secrets.join(GITHUB_USER_FILE)).output().unwrap();
    assert_eq!(String::from_utf8(decrypted.stdout).unwrap().trim(), PLANTED);
    let registry = std::fs::read_to_string(south.path().join("agents/credentials.toml")).unwrap();
    assert!(registry.contains("expires = \"2027-01-02\""), "{registry}");

    let (refusing, _) = fake_upstream(|_, _| (401, json!({ "message": "Bad credentials" })));
    let refused = Copied { text: PLANTED.to_owned(), cleared: std::cell::Cell::new(false) };
    let again = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(again.path().join("secrets")).unwrap();
    std::fs::create_dir_all(again.path().join("agents")).unwrap();
    std::fs::copy(south.path().join("agents/credentials.toml"), again.path().join("agents/credentials.toml")).unwrap();
    let error = crate::intake::run(&again.path().join("secrets"), &refusing, &refused, &mut Vec::new()).unwrap_err();
    assert!(error.contains("401") && !error.contains(PLANTED), "{error}");
    assert!(!refused.cleared.get() && !again.path().join("secrets").join(GITHUB_USER_FILE).exists());
}
