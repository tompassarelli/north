use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::http::{Request, Response};
use super::upstream::Upstream;

pub struct Settings {
    pub fresh: Duration,
    pub write_gap: Duration,
    pub default_hold: Duration,
    pub capacity: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self { fresh: Duration::from_secs(20), write_gap: Duration::from_secs(1), default_hold: Duration::from_secs(60), capacity: 2048 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Upstream,
    Fresh,
    Revalidated,
    Stale,
    Held,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Fresh => "fresh",
            Self::Revalidated => "revalidated",
            Self::Stale => "stale",
            Self::Held => "held",
        }
    }
}

type Key = [u8; 32];

struct Entry {
    response: Response,
    etag: Option<String>,
    fetched: Instant,
}

#[derive(Default)]
struct State {
    cache: HashMap<Key, Entry>,
    inflight: HashSet<Key>,
    holds: HashMap<Key, Instant>,
    next_write: Option<Instant>,
}

pub struct Broker {
    upstream: Upstream,
    settings: Settings,
    state: Mutex<State>,
    settled: Condvar,
}

struct Claim<'b> {
    broker: &'b Broker,
    key: Key,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.broker.lock().inflight.remove(&self.key);
        self.broker.settled.notify_all();
    }
}

impl Broker {
    pub fn new(upstream: Upstream, settings: Settings) -> Self {
        Self { upstream, settings, state: Mutex::default(), settled: Condvar::new() }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn handle(&self, request: &Request) -> (Response, Source) {
        if is_read(request) { self.read(request) } else { self.write(request) }
    }

    fn read(&self, request: &Request) -> (Response, Source) {
        let key = key(request);
        let bucket = bucket(request);
        let mut state = self.lock();
        loop {
            let now = Instant::now();
            if let Some(until) = state.holds.get(&bucket).copied().filter(|until| *until > now) {
                return match state.cache.get(&key) {
                    Some(entry) => (entry.response.clone(), Source::Stale),
                    None => (held(until - now), Source::Held),
                };
            }
            if let Some(entry) = state.cache.get(&key).filter(|entry| now - entry.fetched < self.settings.fresh) {
                return (entry.response.clone(), Source::Fresh);
            }
            if state.inflight.insert(key) {
                break;
            }
            state = self.settled.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        let etag = state.cache.get(&key).and_then(|entry| entry.etag.clone());
        drop(state);
        let claim = Claim { broker: self, key };
        let response = self.upstream.send(request, etag.as_deref());
        let mut state = self.lock();
        let now = Instant::now();
        let outcome = if self.hold(&mut state, bucket, &response, now) {
            (response, Source::Upstream)
        } else if let Some(entry) = state.cache.get_mut(&key).filter(|_| response.status == 304) {
            entry.fetched = now;
            (entry.response.clone(), Source::Revalidated)
        } else {
            if response.status == 200 {
                self.store(&mut state, key, &response, now);
            }
            (response, Source::Upstream)
        };
        drop(state);
        drop(claim);
        outcome
    }

    fn write(&self, request: &Request) -> (Response, Source) {
        let bucket = bucket(request);
        let mut state = self.lock();
        let now = Instant::now();
        if let Some(until) = state.holds.get(&bucket).copied().filter(|until| *until > now) {
            return (held(until - now), Source::Held);
        }
        let slot = state.next_write.map_or(now, |next| next.max(now));
        state.next_write = Some(slot + self.settings.write_gap);
        drop(state);
        std::thread::sleep(slot - now);
        let response = self.upstream.send(request, None);
        let mut state = self.lock();
        if !self.hold(&mut state, bucket, &response, Instant::now()) && (200..300).contains(&response.status) {
            state.cache.clear();
        }
        (response, Source::Upstream)
    }

    fn store(&self, state: &mut State, key: Key, response: &Response, now: Instant) {
        if state.cache.len() >= self.settings.capacity
            && let Some(oldest) = state.cache.iter().min_by_key(|(_, entry)| entry.fetched).map(|(key, _)| *key)
        {
            state.cache.remove(&oldest);
        }
        let etag = response.header("etag").map(str::to_owned);
        state.cache.insert(key, Entry { response: response.clone(), etag, fetched: now });
    }

    fn hold(&self, state: &mut State, bucket: Key, response: &Response, now: Instant) -> bool {
        let Some(wait) = hold_for(response, self.settings.default_hold) else {
            return false;
        };
        let until = now + wait;
        let held = state.holds.entry(bucket).or_insert(until);
        *held = (*held).max(until);
        true
    }
}

fn is_read(request: &Request) -> bool {
    match request.method.as_str() {
        "GET" | "HEAD" => true,
        "POST" if request.target.split('?').next() == Some("/graphql") => serde_json::from_slice::<serde_json::Value>(&request.body)
            .ok()
            .and_then(|body| body["query"].as_str().map(|query| !query.trim_start().starts_with("mutation")))
            .unwrap_or(false),
        _ => false,
    }
}

fn bucket(request: &Request) -> Key {
    let path = request.target.split('?').next().unwrap_or("");
    let class = if path == "/graphql" { "graphql" } else if path.starts_with("/search/") { "search" } else { "core" };
    let mut hash = Sha256::new();
    for part in [request.header("host").unwrap_or(""), request.header("authorization").unwrap_or(""), class] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.finalize().into()
}

fn key(request: &Request) -> Key {
    let mut hash = Sha256::new();
    for part in [request.method.as_str(), request.header("host").unwrap_or(""), request.target.as_str(), request.header("accept").unwrap_or(""), request.header("authorization").unwrap_or("")] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.update(&request.body);
    hash.finalize().into()
}

pub fn hold_for(response: &Response, default_hold: Duration) -> Option<Duration> {
    if ![403, 429].contains(&response.status) {
        return None;
    }
    if let Some(seconds) = response.header("retry-after").and_then(|value| value.trim().parse::<u64>().ok()) {
        return Some(Duration::from_secs(seconds.max(1)));
    }
    if response.header("x-ratelimit-remaining") == Some("0") {
        let reset = response.header("x-ratelimit-reset").and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(0);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        return Some(Duration::from_secs(reset.saturating_sub(now).max(1)));
    }
    let secondary = String::from_utf8_lossy(&response.body).to_ascii_lowercase().contains("secondary rate limit");
    (response.status == 429 || secondary).then_some(default_hold)
}

fn held(remaining: Duration) -> Response {
    let seconds = remaining.as_secs() + 1;
    Response::json(429, vec![("Retry-After".to_owned(), seconds.to_string())], &format!("north gh-broker: GitHub asked for a pause; no GitHub calls for {seconds} s"))
}
