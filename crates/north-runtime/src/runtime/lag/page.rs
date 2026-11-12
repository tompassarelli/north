//! One reading of the fleet's lag: the `--json` objects, a scoreboard sample and the proxy pages per episode.
use super::behind::{self, serves_host};
use super::samples::{self, Sample};
use crate::land_offload;
use crate::runtime::failure::{Failure, failed};
use crate::runtime::layout::{self, Layout};
use crate::runtime::pin;
use crate::runtime::update;
use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const KEY: &str = "runtime-lag";
pub(super) const PAGE_AFTER_S: i64 = 300;
const EPISODES: &str = "runtime-lag-episodes.json";
const LOCK: &str = "runtime-lag.lock";
const STARTED: &str = "runtime-lag.started";
const SENDER: &str = "service:north-runtime-lag";
/// The fleet tick starts a sample at most this often, so the file keeps a week at one sample a minute.
const AGAIN: Duration = Duration::from_secs(60);

/// Where the pages of a reading go: the proxy in production, a recorder in tests.
pub(super) trait Pager {
    fn session_lags(&mut self, text: &str) -> bool;
    fn service_lags(&mut self, key: &str, text: &str) -> bool;
    fn service_clears(&mut self, key: &str, text: &str) -> bool;
}

/// The episodes still open across readings: a session lag paged, and each host service paged and not yet cleared.
#[derive(Default, Deserialize, Serialize)]
struct Episodes {
    sessions: bool,
    services: BTreeSet<(String, String)>,
}

impl Episodes {
    fn read(state: &Path) -> Self {
        fs::read_to_string(state.join(EPISODES)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn write(&self, state: &Path) {
        let written = serde_json::to_string(self).map_err(|error| error.to_string()).and_then(|text| fs::write(state.join(EPISODES), text).map_err(|error| error.to_string()));
        if let Err(error) = written {
            eprintln!("north runtime lag: could not record the open episodes: {error}");
        }
    }
}

struct Proxy;

impl Pager for Proxy {
    fn session_lags(&mut self, text: &str) -> bool {
        update::page_proxy(KEY, text)
    }

    fn service_lags(&mut self, key: &str, text: &str) -> bool {
        send(key, text, false)
    }

    fn service_clears(&mut self, key: &str, text: &str) -> bool {
        send(key, text, true)
    }
}

fn send(key: &str, text: &str, cleared: bool) -> bool {
    let mut command = Command::new("north");
    command.args(["fleet", "send", "--from", SENDER, "--key", key]);
    if cleared {
        command.arg("--cleared");
    }
    let delivered = command.arg("proxy").arg(text).stdin(Stdio::null()).status().is_ok_and(|status| status.success());
    if !delivered {
        eprintln!("north runtime lag: the proxy page for {key} was not delivered");
    }
    delivered
}

pub(super) fn objects(layout: &Layout, probed: &[Value]) -> Vec<Value> {
    let checkout = update::checkout();
    behind::objects(&super::ask::seen(layout, probed), &super::ask::landings(layout), pin::now(), |landed, runtime| landed == runtime || crate::runtime::settle::ancestor(&checkout, landed, runtime))
}

fn over(object: &Value) -> bool {
    object["seconds"].as_i64().is_some_and(|seconds| seconds > PAGE_AFTER_S)
}

fn is_service(object: &Value) -> bool {
    serves_host(object["kind"].as_str().unwrap_or_default())
}

/// Reads the lag of every host, pages what newly lags and records one sample.
pub(super) fn conclude(state: &Path, objects: &[Value], at: i64, pager: &mut impl Pager) {
    let Some(worst) = behind::worst(objects) else { return };
    let mut episodes = Episodes::read(state);
    let lagging_sessions: Vec<&Value> = objects.iter().filter(|object| !is_service(object) && over(object)).collect();
    if lagging_sessions.is_empty() {
        episodes.sessions = false;
    } else if !episodes.sessions {
        let worst_session = lagging_sessions.iter().copied().max_by_key(|object| object["seconds"].as_i64()).unwrap_or(worst);
        episodes.sessions = pager.session_lags(&session_text(worst_session, lagging_sessions.len()));
    }
    let mut lagging: BTreeMap<(String, String), &Value> = BTreeMap::new();
    for object in objects.iter().filter(|object| is_service(object) && over(object)) {
        let slot = lagging.entry(service(object)).or_insert(object);
        if object["seconds"].as_i64() > slot["seconds"].as_i64() {
            *slot = object;
        }
    }
    for (pair, object) in &lagging {
        if !episodes.services.contains(pair) && pager.service_lags(&service_key(pair), &service_text(object)) {
            episodes.services.insert(pair.clone());
        }
    }
    let unread: BTreeSet<&str> = objects.iter().filter(|object| object["process"].is_null()).filter_map(|object| object["host"].as_str()).collect();
    let recovered: Vec<(String, String)> = episodes.services.iter().filter(|pair| !lagging.contains_key(*pair) && !unread.contains(pair.0.as_str())).cloned().collect();
    for pair in recovered {
        if pager.service_clears(&service_key(&pair), &format!("north runtime lag: {} on {} is under {PAGE_AFTER_S}s behind again", pair.1, pair.0)) {
            episodes.services.remove(&pair);
        }
    }
    let paged = match is_service(worst) {
        true => episodes.services.contains(&service(worst)),
        false => episodes.sessions,
    };
    if let Some(sample) = Sample::of(worst, at, paged) {
        samples::record(state, &sample);
    }
    episodes.write(state);
}

fn service(object: &Value) -> (String, String) {
    (object["host"].as_str().unwrap_or_default().to_owned(), object["process"].as_str().unwrap_or_default().to_owned())
}

fn service_key((host, process): &(String, String)) -> String {
    format!("{KEY}:{host}:{process}")
}

fn session_text(worst: &Value, over: usize) -> String {
    format!("{}; {over} session process{} over {PAGE_AFTER_S}s; north runtime lag --json lists them", reading(worst), if over == 1 { "" } else { "es" })
}

fn service_text(object: &Value) -> String {
    format!("{}; a host service over {PAGE_AFTER_S}s; north runtime lag --json lists every process", reading(object))
}

fn reading(object: &Value) -> String {
    let runtime = object["runtime"].as_str().map_or_else(|| "an older runtime".to_owned(), |sha| layout::short(sha).to_owned());
    format!("north runtime lag: {} on {} runs {runtime}, {}s behind the newest landing that changed a runtime input", object["process"].as_str().unwrap_or_default(), object["host"].as_str().unwrap_or_default(), object["seconds"])
}

/// One sample of the lag and its pages, run by the settling host's fleet tick in a scope of its own.
pub(super) fn sample(layout: &Layout) -> Result<(), Failure> {
    if !land_offload::settles_fleet_here() {
        return Err(failed(1, "this host does not settle the fleet, so it samples no fleet lag"));
    }
    fs::create_dir_all(&layout.state)?;
    let lock = File::create(layout.state.join(LOCK))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        return Ok(());
    }
    conclude(&layout.state, &objects(layout, &[]), pin::now(), &mut Proxy);
    Ok(())
}

/// Starts one sample in a scope of its own at most once a minute, so the tick never waits on a slow host.
pub fn publish() -> Vec<String> {
    if !land_offload::settles_fleet_here() {
        return Vec::new();
    }
    let layout = Layout::from_env();
    let marker = layout.state.join(STARTED);
    let recent = fs::metadata(&marker).and_then(|metadata| metadata.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < AGAIN);
    if recent {
        return Vec::new();
    }
    let started = fs::create_dir_all(&layout.state)
        .and_then(|()| fs::write(&marker, ""))
        .map_err(|error| error.to_string())
        .and_then(|()| std::env::current_exe().map_err(|error| error.to_string()))
        .and_then(|north| {
            crate::runtime::follow::own_scope(north).args(["runtime", "lag", "--sample"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().map_err(|error| error.to_string())
        });
    match started {
        Ok(_) => Vec::new(),
        Err(error) => vec![format!("lag: not sampled: {error}")],
    }
}
