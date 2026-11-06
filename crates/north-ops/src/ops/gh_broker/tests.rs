use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::broker::{Broker, Settings};
use std::io::{BufReader, Read, Write};

use super::http::{Request, Response, read_headers, read_line, read_request, status_code, write_response};
use crate::platform::gh_broker_route::Place;
use super::upstream::Upstream;

const ISSUES: &str = "/repos/o/r/issues";
const ETAG: &str = "\"v1\"";

#[derive(Default)]
struct Ledger {
    calls: AtomicUsize,
    charged: AtomicUsize,
    seen: Mutex<Vec<String>>,
}

fn reply(request: &Request) -> Response {
    let header = |name: &str, value: String| (name.to_owned(), value);
    match request.target.as_str() {
        ISSUES if request.header("if-none-match") == Some(ETAG) => Response { status: 304, headers: vec![header("ETag", ETAG.into())], body: Vec::new() },
        ISSUES | "/repos/o/r/pulls" => {
            std::thread::sleep(Duration::from_millis(300));
            Response { status: 200, headers: vec![header("ETag", ETAG.into())], body: format!("[{{\"from\":\"{}\"}}]", request.target).into_bytes() }
        }
        "/graphql" => Response { status: 200, headers: Vec::new(), body: b"{\"data\":{}}".to_vec() },
        "/limited/retry-after" => Response { status: 429, headers: vec![header("Retry-After", "120".into())], body: Vec::new() },
        "/limited/secondary" => Response { status: 403, headers: Vec::new(), body: b"{\"message\":\"You have exceeded a secondary rate limit.\"}".to_vec() },
        "/limited/primary" => {
            let reset = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 300;
            Response { status: 403, headers: vec![header("X-RateLimit-Remaining", "0".into()), header("X-RateLimit-Reset", reset.to_string())], body: b"{\"message\":\"API rate limit exceeded\"}".to_vec() }
        }
        _ => Response { status: 201, headers: Vec::new(), body: b"{}".to_vec() },
    }
}

fn fake_github() -> (String, Arc<Ledger>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let ledger = Arc::new(Ledger::default());
    let counted = Arc::clone(&ledger);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let ledger = Arc::clone(&counted);
            std::thread::spawn(move || {
                let request = read_request(&stream).unwrap();
                let response = reply(&request);
                ledger.calls.fetch_add(1, Ordering::SeqCst);
                if response.status != 304 {
                    ledger.charged.fetch_add(1, Ordering::SeqCst);
                }
                ledger.seen.lock().unwrap().push(format!("{} {} {}", request.method, request.target, request.header("if-none-match").unwrap_or("-")));
                let _ = write_response(&stream, &response, "fake");
            });
        }
    });
    (address, ledger)
}

struct Harness {
    socket: PathBuf,
    github: Arc<Ledger>,
    _root: tempfile::TempDir,
}

impl Harness {
    fn start(fresh: Duration) -> Self {
        let (api, github) = fake_github();
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("broker.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let settings = Settings { fresh, write_gap: Duration::from_millis(10), ..Settings::default() };
        super::spawn_server(listener, Broker::new(Upstream::new(&api), settings));
        Self { socket, github, _root: root }
    }

    fn send(&self, method: &str, target: &str, body: &str) -> Response {
        self.send_as("token session", method, target, body)
    }

    fn send_as(&self, token: &str, method: &str, target: &str, body: &str) -> Response {
        self.send_with(token, None, method, target, body)
    }

    fn send_with(&self, token: &str, etag: Option<&str>, method: &str, target: &str, body: &str) -> Response {
        let stream = UnixStream::connect(&self.socket).unwrap();
        let mut headers = vec![("Host".to_owned(), "api.github.com".to_owned()), ("Authorization".to_owned(), token.to_owned()), ("Accept-Encoding".to_owned(), "gzip".to_owned())];
        headers.extend(etag.map(|etag| ("If-None-Match".to_owned(), etag.to_owned())));
        write_request(&stream, &Request { method: method.to_owned(), target: target.to_owned(), headers, body: body.as_bytes().to_vec() }).unwrap();
        read_response(&stream).unwrap()
    }

    fn get(&self, target: &str) -> Response {
        self.send("GET", target, "")
    }

    fn calls(&self) -> usize {
        self.github.calls.load(Ordering::SeqCst)
    }
}

fn source(response: &Response) -> &str {
    response.header("x-north-broker").unwrap_or("")
}

#[test]
fn two_sessions_identical_reads_cost_one_github_call() {
    let broker = Arc::new(Harness::start(Duration::from_secs(20)));
    let sessions: Vec<_> = (0..2).map(|_| { let broker = Arc::clone(&broker); std::thread::spawn(move || broker.get(ISSUES)) }).collect();
    let answers: Vec<Response> = sessions.into_iter().map(|session| session.join().unwrap()).collect();
    for answer in &answers {
        assert_eq!((answer.status, answer.body.as_slice()), (200, b"[{\"from\":\"/repos/o/r/issues\"}]".as_slice()));
    }
    assert_eq!(broker.calls(), 1, "{:?}", broker.github.seen.lock().unwrap());
    let third = broker.get(ISSUES);
    assert_eq!((third.status, source(&third)), (200, "fresh"));
    assert_eq!(broker.calls(), 1);
    broker.get("/repos/o/r/pulls");
    assert_eq!(broker.calls(), 2, "a different read is its own call");
}

#[test]
fn a_repeat_read_is_answered_from_an_etag_304_costing_no_rate_limit() {
    let broker = Harness::start(Duration::ZERO);
    let first = broker.get(ISSUES);
    assert_eq!((first.status, source(&first)), (200, "upstream"));
    let again = broker.get(ISSUES);
    assert_eq!((again.status, source(&again), again.body.as_slice()), (200, "revalidated", first.body.as_slice()));
    assert_eq!(*broker.github.seen.lock().unwrap(), [format!("GET {ISSUES} -"), format!("GET {ISSUES} {ETAG}")]);
    assert_eq!((broker.calls(), broker.github.charged.load(Ordering::SeqCst)), (2, 1), "the 304 spends no rate limit");
    let fresh = Harness::start(Duration::ZERO);
    let kept = fresh.send_with("token mirror", Some(ETAG), "GET", ISSUES, "");
    assert_eq!((kept.status, fresh.github.charged.load(Ordering::SeqCst)), (304, 0), "a caller's own ETag still revalidates when the broker holds no copy");
}

#[test]
fn retry_after_holds_every_call_with_that_token_until_it_passes_and_no_other_token() {
    let broker = Harness::start(Duration::from_secs(20));
    assert_eq!(broker.get("/limited/retry-after").status, 429);
    for (method, target) in [("GET", ISSUES), ("POST", "/repos/o/r/issues/1/comments")] {
        let held = broker.send(method, target, "{}");
        assert_eq!((held.status, source(&held)), (429, "held"), "{method} {target}");
        let wait: u64 = held.header("retry-after").unwrap().parse().unwrap();
        assert!((100..=121).contains(&wait), "{wait}");
    }
    assert_eq!(broker.calls(), 1, "nothing reaches GitHub during the hold");
    let other = broker.send_as("token personal", "GET", ISSUES, "");
    assert_eq!((other.status, source(&other)), (200, "upstream"), "one exhausted token holds no other token on the host");
}

#[test]
fn secondary_and_primary_limits_hold_and_serve_stored_reads_stale() {
    for limit in ["/limited/secondary", "/limited/primary"] {
        let broker = Harness::start(Duration::ZERO);
        let stored = broker.get(ISSUES);
        assert_eq!(broker.get(limit).status, 403);
        let stale = broker.get(ISSUES);
        assert_eq!((stale.status, source(&stale), stale.body.as_slice()), (200, "stale", stored.body.as_slice()), "{limit}");
        let held = broker.get("/repos/o/r/pulls");
        assert_eq!((held.status, source(&held)), (429, "held"), "{limit}");
        assert_eq!(broker.calls(), 2, "{limit}");
    }
}

#[test]
fn graphql_queries_are_shared_reads_and_a_mutation_passes_through_and_clears_stored_reads() {
    let broker = Harness::start(Duration::from_secs(20));
    let query = r#"{"query":"query { viewer { login } }"}"#;
    broker.send("POST", "/graphql", query);
    assert_eq!(source(&broker.send("POST", "/graphql", query)), "fresh");
    let mutation = r#"{"query":"mutation { addComment(input: {}) { clientMutationId } }"}"#;
    assert_eq!(source(&broker.send("POST", "/graphql", mutation)), "upstream");
    assert_eq!(source(&broker.send("POST", "/graphql", query)), "upstream");
    assert_eq!(broker.calls(), 3);
}

#[test]
fn the_session_gh_config_points_gh_at_the_broker_and_keeps_its_login() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("gh");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("config.yml"), "git_protocol: https\nhttp_unix_socket: /elsewhere\n").unwrap();
    let place = Place::at(root.path().join("broker"));
    let directory = place.gh_config(&source).unwrap();
    let config = std::fs::read_to_string(directory.join("config.yml")).unwrap();
    assert_eq!(config, format!("git_protocol: https\nhttp_unix_socket: {}\n", place.socket().display()));
    assert_eq!(std::fs::read_link(directory.join("hosts.yml")).unwrap(), source.join("hosts.yml"));
    assert_eq!(place.gh_config(&source).unwrap(), directory);
}

fn write_request(mut stream: impl Write, request: &Request) -> std::io::Result<()> {
    let mut head = format!("{} {} HTTP/1.1\r\n", request.method, request.target);
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", request.body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&request.body)
}

fn read_response(stream: impl Read) -> std::io::Result<Response> {
    let mut reader = BufReader::new(stream);
    let status = status_code(&read_line(&mut reader)?).unwrap();
    let headers = read_headers(&mut reader)?;
    let mut body = Vec::new();
    reader.read_to_end(&mut body)?;
    Ok(Response { status, headers, body })
}

#[test]
fn the_resident_broker_claims_the_users_gh_config_and_releases_only_its_own_line_while_no_broker_answers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("gh");
    std::fs::create_dir_all(&source).unwrap();
    let original = "git_protocol: https\nhttp_unix_socket:\nprompt: enabled\n";
    std::fs::write(source.join("config.yml"), original).unwrap();
    let place = Place::at(root.path().join("broker"));
    place.claim_user_gh(&source).unwrap();
    let claimed = std::fs::read_to_string(source.join("config.yml")).unwrap();
    assert_eq!(claimed, format!("git_protocol: https\nprompt: enabled\nhttp_unix_socket: {}\n", place.socket().display()));
    let serving = UnixListener::bind(place.socket()).unwrap();
    assert!(!place.release_user_gh(&source).unwrap(), "a broker still answers on the socket");
    assert_eq!(std::fs::read_to_string(source.join("config.yml")).unwrap(), claimed);
    drop(serving);
    std::fs::remove_file(place.socket()).unwrap();
    assert!(place.release_user_gh(&source).unwrap());
    assert_eq!(std::fs::read_to_string(source.join("config.yml")).unwrap(), "git_protocol: https\nprompt: enabled\n");
    let elsewhere = "git_protocol: https\nhttp_unix_socket: /elsewhere.sock\n";
    std::fs::write(source.join("config.yml"), elsewhere).unwrap();
    assert!(!place.release_user_gh(&source).unwrap());
    assert_eq!(std::fs::read_to_string(source.join("config.yml")).unwrap(), elsewhere);
}
