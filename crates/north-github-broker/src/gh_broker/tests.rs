use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::broker::{Broker, Settings};
use std::io::{BufReader, Read, Write};

use super::http::{Request, Response, read_headers, read_line, read_request, status_code, write_response};
use north_base::platform::gh_broker_route::Place;
use super::upstream::Upstream;

const ISSUES: &str = "/repos/o/r/issues";
const ETAG: &str = "\"v1\"";

#[derive(Default)]
struct Ledger {
    arrived: AtomicUsize,
    calls: AtomicUsize,
    charged: AtomicUsize,
    seen: Mutex<Vec<String>>,
    held: AtomicBool,
}

fn reply(request: &Request, held: &AtomicBool) -> Response {
    let header = |name: &str, value: String| (name.to_owned(), value);
    match request.target.as_str() {
        ISSUES if request.header("if-none-match") == Some(ETAG) => {
            north_base::platform::wait::until(Duration::from_secs(60), || !held.load(Ordering::SeqCst));
            Response { status: 304, headers: vec![header("ETag", ETAG.into())], body: Vec::new() }
        }
        ISSUES | "/repos/o/r/pulls" => {
            north_base::platform::wait::until(Duration::from_secs(60), || !held.load(Ordering::SeqCst));
            Response { status: 200, headers: vec![header("ETag", ETAG.into())], body: format!("[{{\"from\":\"{}\"}}]", request.target).into_bytes() }
        }
        "/graphql" => Response { status: 200, headers: Vec::new(), body: b"{\"data\":{}}".to_vec() },
        "/repos/o/uninstalled" if request.header("authorization").is_some_and(|token| token.contains("ghs_")) => Response { status: 404, headers: Vec::new(), body: Vec::new() },
        "/whoami" | "/repos/o/uninstalled" => Response { status: 200, headers: Vec::new(), body: request.header("authorization").unwrap_or("").as_bytes().to_vec() },
        "/limited/retry-after" => Response { status: 429, headers: vec![header("Retry-After", "120".into())], body: Vec::new() },
        "/limited/secondary" => Response { status: 403, headers: Vec::new(), body: b"{\"message\":\"You have exceeded a secondary rate limit.\"}".to_vec() },
        "/limited/primary" => {
            let reset = 4_000_000_000_u64;
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
                ledger.arrived.fetch_add(1, Ordering::SeqCst);
                let response = reply(&request, &ledger.held);
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
    activity: Arc<super::server::Activity>,
    served: Option<std::thread::JoinHandle<super::server::Ended>>,
    listener: UnixListener,
    api: String,
    _root: tempfile::TempDir,
}

impl Harness {
    fn start(fresh: Duration) -> Self {
        let (api, github) = fake_github();
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("broker.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let frozen = (Settings::default().clock)();
        let settings = Settings { fresh, write_gap: Duration::from_millis(10), clock: Arc::new(move || frozen), ..Settings::default() };
        let kept = listener.try_clone().unwrap();
        let (activity, served) = super::spawn_server(listener, Broker::new(Upstream::new(&api), settings));
        Self { socket, github, activity, served: Some(served), listener: kept, api, _root: root }
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
    broker.github.held.store(true, Ordering::SeqCst);
    let sessions: Vec<_> = (0..2).map(|_| { let broker = Arc::clone(&broker); std::thread::spawn(move || broker.get(ISSUES)) }).collect();
    assert!(north_base::platform::wait::until(Duration::from_secs(60), || broker.activity.open_calls() == 2), "both sessions reached the broker");
    broker.github.held.store(false, Ordering::SeqCst);
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
fn a_write_clearing_the_cache_mid_revalidation_never_hands_the_caller_a_304() {
    let broker = Arc::new(Harness::start(Duration::ZERO));
    broker.get(ISSUES);
    broker.github.held.store(true, Ordering::SeqCst);
    let reader = { let broker = Arc::clone(&broker); std::thread::spawn(move || broker.get(ISSUES)) };
    assert!(north_base::platform::wait::until(Duration::from_secs(60), || broker.github.arrived.load(Ordering::SeqCst) == 2), "the revalidation reached GitHub");
    assert_eq!(broker.send("POST", "/repos/o/r/issues/1/comments", "{}").status, 201);
    broker.github.held.store(false, Ordering::SeqCst);
    let answer = reader.join().unwrap();
    assert_eq!((answer.status, answer.body.as_slice()), (200, b"[{\"from\":\"/repos/o/r/issues\"}]".as_slice()));
    assert_eq!(broker.github.seen.lock().unwrap().last().unwrap(), &format!("GET {ISSUES} -"), "the broker refetched without a condition");
}

#[test]
fn retry_after_holds_every_call_with_that_token_until_it_passes_and_no_other_token() {
    let broker = Harness::start(Duration::from_secs(20));
    assert_eq!(broker.get("/limited/retry-after").status, 429);
    for (method, target) in [("GET", ISSUES), ("POST", "/repos/o/r/issues/1/comments")] {
        let held = broker.send(method, target, "{}");
        assert_eq!((held.status, source(&held)), (429, "held"), "{method} {target}");
        let wait: u64 = held.header("retry-after").unwrap().parse().unwrap();
        assert_eq!(wait, 121);
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
fn the_brokers_gh_config_edits_keep_the_login_and_release_only_its_own_line_while_no_broker_answers() {
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
    let sessions = root.path().join("session-gh");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("config.yml"), "git_protocol: https\nhttp_unix_socket: /elsewhere\n").unwrap();
    let directory = place.gh_config(&sessions).unwrap();
    assert_eq!(std::fs::read_to_string(directory.join("config.yml")).unwrap(), format!("git_protocol: https\nhttp_unix_socket: {}\n", place.socket().display()));
    assert!(directory.join("hosts.yml").symlink_metadata().is_err(), "a session's gh config must not reach the operator's login");
    assert_eq!(place.gh_config(&sessions).unwrap(), directory);
}

#[test]
fn a_resident_broker_finishes_its_open_calls_then_hands_queued_ones_to_the_next_runtime_when_the_runtime_moves() {
    let mut broker = Harness::start(Duration::from_secs(20));
    broker.github.held.store(true, Ordering::SeqCst);
    let state = tempfile::tempdir().unwrap();
    let (old, new) = ("a".repeat(40), "b".repeat(40));
    for sha in [&old, &new] {
        std::fs::create_dir_all(state.path().join("runtimes").join(sha).join("bin")).unwrap();
    }
    let link = state.path().join("current");
    std::os::unix::fs::symlink(format!("runtimes/{old}"), &link).unwrap();
    let executable = state.path().join("runtimes").join(&old).join("bin/north");
    let socket = broker.socket.clone();
    let held = {
        let socket = socket.clone();
        std::thread::spawn(move || {
            let stream = UnixStream::connect(&socket).unwrap();
            let headers = vec![("Host".to_owned(), "api.github.com".to_owned()), ("Authorization".to_owned(), "token session".to_owned())];
            write_request(&stream, &Request { method: "GET".to_owned(), target: ISSUES.to_owned(), headers, body: Vec::new() }).unwrap();
            read_response(&stream).unwrap()
        })
    };
    north_base::platform::wait::until(Duration::from_secs(60), || broker.activity.idle_for().is_none());
    let place = Place::at(socket.parent().unwrap().to_path_buf());
    super::lifetime::watch_runtime(&place, Arc::clone(&broker.activity), true, north_runtime_follow::Follow::of(link.clone(), Some(executable.clone())));
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{new}"), &link).unwrap();
    north_base::platform::wait::until(Duration::from_secs(60), || broker.activity.is_handing());
    let stream = UnixStream::connect(&socket).unwrap();
    let headers = vec![("Host".to_owned(), "api.github.com".to_owned()), ("Authorization".to_owned(), "token session".to_owned())];
    write_request(&stream, &Request { method: "GET".to_owned(), target: "/whoami".to_owned(), headers, body: Vec::new() }).unwrap();
    let queued = std::thread::spawn(move || read_response(&stream).unwrap());
    let served = broker.served.take().unwrap();
    assert!(!served.is_finished(), "the broker keeps serving the request it holds after the link moved");
    broker.github.held.store(false, Ordering::SeqCst);
    assert!(matches!(served.join().unwrap(), super::server::Ended::HandingOver { finished: true }), "the broker hands its socket over once the call open at the move is answered");
    assert_eq!(held.join().unwrap().status, 200, "the request open at the promotion is answered, not cut");
    assert!(!queued.is_finished(), "the old runtime takes no call after the move, so its drain does not wait on calls that keep arriving");
    let _adopted = super::spawn_server(broker.listener.try_clone().unwrap(), Broker::new(Upstream::new(&broker.api), Settings::default()));
    assert_eq!(queued.join().unwrap().status, 200, "a call that connected during the hand-over is answered by the next runtime, not reset");
    let next = north_runtime_follow::Follow::of(link, Some(executable)).next().unwrap();
    assert_eq!(next, state.path().canonicalize().unwrap().join("runtimes").join(&new).join("bin/north"), "the restarted broker runs the promoted runtime");
}

#[test]
fn a_services_rest_read_spends_the_app_token_and_falls_back_to_its_own_login_where_the_app_cannot_read() {
    let (api, _github) = fake_github();
    let broker = Broker::new(Upstream::new(&api), Settings::default());
    let app = super::app_reads::AppReads::with(Box::new(|| Some("ghs_app".to_owned())));
    let token = |unit: Option<&str>, method: &str, target: &str| {
        let headers = vec![("Host".to_owned(), "api.github.com".to_owned()), ("Authorization".to_owned(), "token gho_personal".to_owned())];
        let caller = super::calls::Caller { name: "lane-gc.service".to_owned(), cgroup: unit.map(|unit| format!("0::/user.slice/user-1000.slice/user@1000.service/{unit}\n")), ours: true };
        let (sent, response, _) = app.serve(&broker, &caller, Request { method: method.to_owned(), target: target.to_owned(), headers, body: Vec::new() });
        (response.status, sent.header("authorization").unwrap_or("").to_owned())
    };
    assert_eq!(token(Some("lane-gc.service"), "GET", "/whoami"), (200, "token ghs_app".to_owned()));
    assert_eq!(token(Some("north-land@north.service"), "GET", "/whoami"), (200, "token ghs_app".to_owned()), "a repository's lander borrows");
    assert_eq!(token(Some("run-p1234-i5678.service"), "GET", "/whoami"), (200, "token gho_personal".to_owned()), "a transient unit borrows nothing");
    assert_eq!(token(Some("north-session-77c49ab1.scope"), "GET", "/whoami"), (200, "token gho_personal".to_owned()), "a caller whose environment names a service but whose cgroup does not borrows nothing");
    assert_eq!(token(Some("north-worker-outside-4142672.service"), "GET", "/whoami"), (200, "token gho_personal".to_owned()), "a service outside the allowlist borrows nothing");
    assert_eq!(token(None, "GET", "/whoami"), (200, "token gho_personal".to_owned()), "a peer whose cgroup was not held borrows nothing");
    assert_eq!(token(Some("app-niri-ghostty-1.scope"), "GET", "/whoami"), (200, "token gho_personal".to_owned()), "an interactive shell keeps its own login");
    assert_eq!(token(Some("lane-gc.service"), "POST", "/repos/o/r/issues/1/comments"), (201, "token gho_personal".to_owned()), "a write keeps its author");
    assert_eq!(token(Some("lane-gc.service"), "GET", "/repos/o/uninstalled"), (200, "token gho_personal".to_owned()), "a repository the app cannot read is read with the unit's own login");
}

#[test]
fn a_worker_scope_callers_call_carries_a_credential_added_outside_its_domain() {
    let (api, _github) = fake_github();
    let broker = Broker::new(Upstream::new(&api), Settings::default());
    let app = super::app_reads::AppReads::with(Box::new(|| Some("ghs_minted_outside".to_owned())));
    let sent = |scope: &str, method: &str, credential: Option<&str>| {
        let mut headers = vec![("Host".to_owned(), "api.github.com".to_owned())];
        headers.extend(credential.map(|credential| ("Authorization".to_owned(), format!("token {credential}"))));
        let caller = super::calls::Caller { name: "north-1397-worker".to_owned(), cgroup: Some(format!("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/{scope}\n")), ours: false };
        let (sent, response, _) = app.serve(&broker, &caller, Request { method: method.to_owned(), target: "/whoami".to_owned(), headers, body: Vec::new() });
        (response.status, sent.header("authorization").unwrap_or("").to_owned())
    };
    let placeholder = north_base::platform::gh_broker_route::CREDENTIAL_ADDED_BY_BROKER;
    const WORKER: &str = "north-session-north-1397-worker.scope";
    assert_eq!(sent(WORKER, "GET", Some(placeholder)), (200, "token ghs_minted_outside".to_owned()), "a worker whose gh holds only the placeholder reads with the broker's token");
    assert_eq!(sent(WORKER, "POST", None), (200, "token ghs_minted_outside".to_owned()), "a worker's write that carries no credential gets the broker's token");
    assert_eq!(sent(WORKER, "GET", Some("ghs_own")), (200, "token ghs_own".to_owned()), "a worker's own credential is sent as it is");
    assert_eq!(sent("app-niri-ghostty-1.scope", "GET", None), (200, String::new()), "a caller outside a worker scope gets no credential added");
    assert_eq!(sent("north-session-lane-deepseek-1.scope", "GET", Some(placeholder)), (200, format!("token {placeholder}")), "a lane-only session, refused every GitHub token (south#104), gets none added");
}

#[test]
fn a_confined_caller_borrows_no_credential_from_a_kept_socket() {
    let (api, _github) = fake_github();
    let broker = Broker::new(Upstream::new(&api), Settings::default());
    let app = super::app_reads::AppReads::with(Box::new(|| Some("ghs_app".to_owned())));
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let sent = |confined: bool| {
        let mut command = std::process::Command::new(if confined { "unshare" } else { "env" });
        command.args(if confined { &["--user", "curl"][..] } else { &["curl"][..] });
        let mut curl = command.args(["-sS", "-o", "/dev/null", "--unix-socket"]).arg(&socket).args(["-H", "Authorization: token gho_stand_in", "http://api.github.com/whoami"]).spawn().unwrap();
        let (stream, _) = listener.accept().unwrap();
        let held = super::calls::caller(&stream);
        let caller = super::calls::Caller { cgroup: Some("0::/user.slice/user-1000.slice/user@1000.service/lane-gc.service\n".to_owned()), ..held };
        let (sent, response, source) = app.serve(&broker, &caller, read_request(&stream).unwrap());
        write_response(&stream, &response, source.name()).unwrap();
        assert!(curl.wait().unwrap().success());
        sent.header("authorization").unwrap_or("").to_owned()
    };
    assert_eq!(sent(false), "token ghs_app", "a borrowing service in the broker's own user namespace reads with the app token");
    assert_eq!(sent(true), "token gho_stand_in", "the same unit from another user namespace keeps the stand-in credential it presented");
}
