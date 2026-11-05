use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ring::signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

use super::*;

const NOW: i64 = 1_800_000_000;

struct Seen {
    path: String,
    jwt: String,
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
                let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let (mut jwt, mut length) = (String::new(), 0);
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(": ").unwrap();
                    match name.to_ascii_lowercase().as_str() {
                        "authorization" => jwt = value.trim_start_matches("Bearer ").to_owned(),
                        "content-length" => length = value.parse().unwrap(),
                        _ => {}
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let mut log = log.lock().unwrap();
                log.push(Seen { path, jwt, body: String::from_utf8(body).unwrap() });
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
        Host { secrets: self.scratch.path().join("secrets"), runtime: Some(self.run()), api: server.api.clone(), decrypt: Box::new(move |_| Some(decrypted.clone())) }
    }

    fn run(&self) -> PathBuf {
        self.scratch.path().join("run")
    }

    fn public_key(&self) -> Vec<u8> {
        signing_key(&self.pem).unwrap().public_key().as_ref().to_vec()
    }
}

fn request(words: &[&str]) -> Request {
    match parse(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).unwrap() {
        Parsed::Mint(request) => request,
        Parsed::Help => panic!("help"),
    }
}

fn claims(jwt: &str, public_key: &[u8]) -> Value {
    let (signed, signature) = jwt.rsplit_once('.').unwrap();
    let signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key).verify(signed.as_bytes(), &signature).unwrap();
    let (head, body) = signed.split_once('.').unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&URL_SAFE_NO_PAD.decode(head).unwrap()).unwrap(), json!({ "alg": "RS256", "typ": "JWT" }));
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).unwrap()).unwrap()
}

#[test]
fn the_default_app_signs_caches_renews_and_scopes_tokens() {
    let server = Server::start();
    let fixture = Fixture::new("github-app.yaml");
    let host = fixture.host(&server, json!({ "app_id": 123, "client_id": "Iv1.test", "installation_id": 42, "private_key": fixture.pem }));
    let cache = fixture.run().join("gh-app-token");

    assert_eq!(mint(&request(&[]), &host, NOW).unwrap(), "ghs_test_1");
    {
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].path, "/app/installations/42/access_tokens");
        assert_eq!(seen[0].body, "");
        let claims = claims(&seen[0].jwt, &fixture.public_key());
        assert_eq!(claims["iss"], "123");
        assert_eq!(claims["iat"], NOW - 60);
        assert_eq!(claims["exp"], NOW + 540);
    }
    assert_eq!(fs::metadata(&cache).unwrap().permissions().mode() & 0o777, 0o600);

    assert_eq!(mint(&request(&[]), &host, NOW).unwrap(), "ghs_test_1");
    assert_eq!(server.requests(), 1);

    assert_eq!(mint(&request(&[]), &host, NOW + 3600 - 240).unwrap(), "ghs_test_2");
    assert_eq!(server.requests(), 2);

    assert_eq!(mint(&request(&["--repos", "north,south"]), &host, NOW).unwrap(), "ghs_test_3");
    let body: Value = serde_json::from_str(&server.seen.lock().unwrap()[2].body).unwrap();
    assert_eq!(body, json!({ "repositories": ["north", "south"], "permissions": { "contents": "write", "metadata": "read" } }));
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(&cache).unwrap()).unwrap()["token"], "ghs_test_2");
    assert_eq!(mint(&request(&["--repos", "north"]), &host, NOW).unwrap(), "ghs_test_4");
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

#[test]
fn the_lander_app_reads_its_own_secret_and_cache() {
    let server = Server::start();
    let fixture = Fixture::new("nexus/north-lander.yaml");
    let host = fixture.host(&server, json!({ "app_id": 5603348, "installation_id": 178367738, "pem": fixture.pem }));

    assert_eq!(mint(&request(&["--app", "lander"]), &host, NOW).unwrap(), "ghs_test_1");
    {
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].path, "/app/installations/178367738/access_tokens");
        assert_eq!(claims(&seen[0].jwt, &fixture.public_key())["iss"], "5603348");
    }
    assert_eq!(fs::metadata(fixture.run().join("gh-app-token-lander")).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(!fixture.run().join("gh-app-token").exists());
    assert_eq!(mint(&request(&["--app", "lander"]), &host, NOW).unwrap(), "ghs_test_1");
    assert_eq!(server.requests(), 1);

    assert!(mint(&request(&[]), &host, NOW).unwrap_err().starts_with("missing "));
}

#[test]
fn arguments_name_a_known_app_and_at_least_one_repository() {
    let words = |words: &[&str]| parse(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>());
    assert_eq!(words(&["--app", "nosuch"]), Err("unknown app nosuch".to_owned()));
    assert_eq!(words(&["--repos", ","]), Err("--repos needs at least one repository name".to_owned()));
    assert!(words(&["--app"]).is_err());
    assert_eq!(words(&["--help"]), Ok(Parsed::Help));
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
fn permissions_take_a_json_object_of_levels_and_need_repositories() {
    let words = |words: &[&str]| parse(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>());
    assert_eq!(words(&["--permissions", r#"{"issues":"write"}"#]), Err("--permissions needs --repos".to_owned()));
    for malformed in ["write", r#"["contents"]"#, r#"{"contents":1}"#] {
        assert_eq!(words(&["--repos", "north", "--permissions", malformed]), Err("--permissions takes a JSON object of permission levels".to_owned()));
    }
    assert_eq!(request(&["--repos", "north"]).permissions, default_permissions());
}
