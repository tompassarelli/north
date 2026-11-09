use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use north_base::foundation::clock::Timestamp;
use north_base::platform::broker_wire::{Request, Response, read_request, write_response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::credentials::Source;
use crate::github::{Minted, Minter, app};
use crate::policy::{Caller, Permissions, Policy, Rules};
use crate::proxy::{Provider, providers, split};

const REUSE_MARGIN: i64 = 300;
const READ_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer {
    pub uid: u32,
    pub pid: i32,
}

#[derive(Default)]
struct State {
    held: HashMap<String, Held>,
    reused: HashMap<String, String>,
}

struct Held {
    uid: u32,
    app: String,
    minted: Minted,
}

pub struct Broker {
    policy: Policy,
    minter: Minter,
    providers: Vec<Provider>,
    state: Mutex<State>,
    pub active: AtomicUsize,
    pub last: Mutex<Instant>,
}

impl Broker {
    pub fn new(policy: Policy, source: Source) -> Self {
        Self::with(policy, Minter::new(source), providers())
    }

    pub fn with(policy: Policy, minter: Minter, providers: Vec<Provider>) -> Self {
        Self { policy, minter, providers, state: Mutex::default(), active: AtomicUsize::new(0), last: Mutex::new(Instant::now()) }
    }

    pub fn serve(self: &Arc<Self>, listener: &UnixListener) {
        for stream in listener.incoming().flatten() {
            let broker = Arc::clone(self);
            std::thread::spawn(move || broker.connection(&stream));
        }
    }

    fn connection(&self, stream: &UnixStream) {
        self.active.fetch_add(1, Ordering::SeqCst);
        let _ = stream.set_read_timeout(Some(READ_LIMIT));
        let answer = match (peer(stream), read_request(stream)) {
            (Some(peer), Ok(request)) => self.handle(peer, &request, Timestamp::now().unix_seconds()),
            (None, _) => Response::refusal(403, "the caller's uid could not be read from the socket"),
            (_, Err(error)) => Response::refusal(400, &format!("unreadable request: {error}")),
        };
        let _ = write_response(stream, &answer);
        *self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
        self.active.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn handle(&self, peer: Peer, request: &Request, now: i64) -> Response {
        let Some(caller) = self.policy.caller(peer.uid) else {
            log(peer, "-", &format!("{} {}", request.method, route(&request.target)), "refused: no policy for this uid");
            return Response::refusal(403, &format!("uid {} has no broker policy", peer.uid));
        };
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let (answer, what) = match (request.method.as_str(), request.target.as_str()) {
            ("POST", "/github/token") => self.github_token(peer, caller, &body, now),
            ("POST", "/github/inspect") => (self.inspect(peer, &body), "github inspect".to_owned()),
            (_, target) => self.proxy(caller, request, target),
        };
        let verdict = if answer.status / 100 == 2 { "granted".to_owned() } else { format!("refused: {}", error_of(&answer)) };
        log(peer, &what, "", &verdict);
        answer
    }

    fn github_token(&self, peer: Peer, caller: &Caller, body: &Value, now: i64) -> (Response, String) {
        let app_name = body["app"].as_str().unwrap_or_default();
        let repositories: Vec<String> = body["repositories"].as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
        let permissions: Permissions = body["permissions"].as_object().into_iter().flatten().filter_map(|(name, level)| Some((name.clone(), level.as_str()?.to_owned()))).collect();
        let what = format!("github token app={app_name} repositories={} permissions={}", repositories.join(","), permissions.iter().map(|(name, level)| format!("{name}:{level}")).collect::<Vec<_>>().join(","));
        let granted = (|| {
            let Some(application) = app(app_name) else { return Err((400, format!("unknown app {app_name:?}"))) };
            let ceiling = caller.github.get(app_name).ok_or_else(|| (403, format!("uid {} may not receive {app_name} tokens", peer.uid)))?;
            let scope = ceiling.scope(app_name, &repositories, &permissions).map_err(|why| (403, why))?;
            let key = format!("{}|{app_name}|{}|{:?}", peer.uid, scope.repositories.join(","), scope.permissions);
            if let Some(minted) = self.reuse(&key, now) {
                return Ok(minted);
            }
            let minted = self.minter.mint(application, &scope, now).map_err(|why| (502, why))?;
            self.remember(&key, peer.uid, app_name, &minted);
            Ok(minted)
        })();
        match granted {
            Ok(minted) => (Response::json(200, &grant_json(&minted, true)), what),
            Err((status, why)) => (Response::refusal(status, &why), what),
        }
    }

    fn reuse(&self, key: &str, now: i64) -> Option<Minted> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let minted = &state.held.get(&digest(state.reused.get(key)?))?.minted;
        (Timestamp::parse_rfc3339(&minted.expires_at)?.unix_seconds() - now > REUSE_MARGIN).then(|| minted.clone())
    }

    fn remember(&self, key: &str, uid: u32, app_name: &str, minted: &Minted) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.held.insert(digest(&minted.token), Held { uid, app: app_name.to_owned(), minted: minted.clone() });
        state.reused.insert(key.to_owned(), minted.token.clone());
    }

    fn inspect(&self, peer: Peer, body: &Value) -> Response {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match body["token"].as_str().and_then(|token| state.held.get(&digest(token))).filter(|held| held.uid == peer.uid) {
            Some(held) => {
                let mut answer = grant_json(&held.minted, false);
                answer["app"] = json!(held.app);
                Response::json(200, &answer)
            }
            None => Response::refusal(403, "the broker granted no such token to this uid"),
        }
    }

    fn proxy(&self, caller: &Caller, request: &Request, target: &str) -> (Response, String) {
        let what = format!("{} {}", request.method, route(target));
        let Some((name, path, query)) = split(target) else { return (Response::refusal(404, "no such route"), what) };
        let Some(provider) = self.providers.iter().find(|provider| provider.name == name) else { return (Response::refusal(404, &format!("no provider {name}")), what) };
        let rules = match caller.proxy.get(name).map(|rules| Rules::new(rules)) {
            Some(Ok(rules)) => rules,
            Some(Err(why)) => return (Response::refusal(500, &why), what),
            None => return (Response::refusal(403, &format!("this caller has no {name} allowlist; {} /{path} is refused", request.method)), what),
        };
        (provider.forward(&self.minter.source, &rules, request, path, query), what)
    }
}

fn grant_json(minted: &Minted, with_token: bool) -> Value {
    let mut answer = json!({ "expires_at": minted.expires_at, "repositories": minted.repositories, "permissions": minted.permissions });
    if with_token {
        answer["token"] = json!(minted.token);
    }
    answer
}

fn digest(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn route(target: &str) -> String {
    target.split('?').next().unwrap_or_default().to_owned()
}

fn error_of(response: &Response) -> String {
    serde_json::from_slice::<Value>(&response.body).ok().and_then(|body| body["error"].as_str().map(str::to_owned)).unwrap_or_else(|| format!("HTTP {}", response.status))
}

fn peer(stream: &UnixStream) -> Option<Peer> {
    let credentials = rustix::net::sockopt::get_socket_peercred(stream).ok()?;
    Some(Peer { uid: credentials.uid.as_raw(), pid: credentials.pid.as_raw_nonzero().get() })
}

fn unit_of(pid: i32) -> String {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    cgroup.lines().filter_map(|line| line.rsplit('/').next()).find(|leaf| leaf.ends_with(".service") || leaf.ends_with(".scope")).unwrap_or("-").to_owned()
}

fn log(peer: Peer, what: &str, extra: &str, verdict: &str) {
    eprintln!("north-broker: uid={} pid={} unit={} {what}{extra} -> {verdict}", peer.uid, peer.pid, unit_of(peer.pid));
}
