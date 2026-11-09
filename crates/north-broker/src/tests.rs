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

use crate::credentials::Source;
use crate::github::Minter;
use crate::policy::Policy;
use crate::proxy::providers;
use crate::server::{Broker, Peer};

const NOW: i64 = 1_800_000_000;
const REAL_TOKEN: &str = "dop_real_secret";
const POLICY: &str = r#"
[[caller]]
uid = 1001
[caller.github.agent]
repositories = ["north", "south"]
permissions = { contents = "write", issues = "write" }
[caller.proxy]
digitalocean = ["GET /v2/droplets", "GET /v2/echo"]
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
            write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
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
    let answer = broker.handle(Peer { uid, pid: 1 }, &request, NOW);
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
    let grant = client.github_token("agent", &["north".to_owned()], &permissions).unwrap();
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
}
