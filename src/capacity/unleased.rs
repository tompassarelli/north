use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::foundation::clock::Timestamp;

const CORE_THRESHOLD: f64 = 1.0;
const STALE_MILLISECONDS: f64 = 90_000.0;
const TAIL_BYTES: u64 = 65536;
const SKIPPED: [&str; 4] = ["session.slice", "native.slice", "init.scope", "agent.slice/agent-capacity.slice"];
const SESSION_PROVIDERS: [&str; 2] = ["claude", "codex"];
const SESSION_FLAGS: [&str; 3] = ["--session-id", "--resume", "resume"];

/// `north capacity unleased`: the watchdog's latest cgroup sample as the CPU nothing leased accounts for, or null when it is missing, unreadable or stale.
pub(crate) fn run() -> u8 {
    println!("{}", read().unwrap_or(Value::Null));
    0
}

fn read() -> Option<Value> {
    let directory = std::env::var_os("CAPACITY_WATCHDOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/watchdog"));
    let sample: Value = serde_json::from_str(&last_line(&directory.join("samples.jsonl"))?).ok()?;
    let now = std::env::var("AGENT_CAPACITY_NOW").map_or_else(|_| Timestamp::now().unix_seconds_f64() * 1000.0, |text| text.parse().unwrap_or(f64::NAN));
    summarize(&sample, now)
}

pub(super) fn summarize(sample: &Value, now: f64) -> Option<Value> {
    let at = sample["at"].as_str().and_then(Timestamp::parse_rfc3339).map_or(f64::NAN, |at| at.unix_seconds_f64() * 1000.0);
    if now - at > STALE_MILLISECONDS {
        return None;
    }
    let cgroups = sample["cgroups"].as_array()?;
    let tops = sample["top"].as_array().map_or(&[][..], Vec::as_slice);
    let heavy: Vec<(String, f64, Value)> = cgroups
        .iter()
        .filter_map(|entry| Some((entry["cgroup"].as_str()?, entry["cores"].as_f64()?)))
        .filter(|(cgroup, cores)| *cores > CORE_THRESHOLD && !skipped(cgroup) && !cgroups.iter().any(|other| other["cgroup"].as_str().is_some_and(|name| name.starts_with(&format!("{cgroup}/")))))
        .map(|(cgroup, cores)| {
            let top: Vec<Value> = tops.iter().filter(|item| item["cgroup"] == cgroup).take(3).map(|item| pick(item, &["pid", "cores", "command"])).collect();
            (cgroup.to_owned(), round(cores), json!({ "cgroup": cgroup, "cores": round(cores), "top": top }))
        })
        .collect();
    let cores_of = |name: &str| cgroups.iter().find(|entry| entry["cgroup"] == name).and_then(|entry| entry["cores"].as_f64()).unwrap_or(0.0);
    let heavy_sum: f64 = heavy.iter().map(|(_, cores, _)| cores).sum();
    let agent_heavy: f64 = heavy.iter().filter(|(cgroup, ..)| cgroup.starts_with("agent.slice/")).map(|(_, cores, _)| cores).sum();
    let residual = (cores_of("agent.slice") - cores_of("agent.slice/agent-capacity.slice") - agent_heavy).max(0.0);
    let sessions: Vec<Value> = tops.iter().filter(|item| item["cores"].as_f64().is_some_and(|cores| cores >= 0.25) && provider(item)).take(5).map(session).collect();
    Some(json!({
        "heavy": heavy.iter().map(|(.., entry)| entry).collect::<Vec<_>>(),
        "sessions": sessions,
        "agentResidualCores": round(residual),
        "cores": round(heavy_sum + residual),
        "windowSeconds": sample["intervalSeconds"].as_f64().map(|seconds| (seconds + 0.5).floor() as i64),
    }))
}

fn last_line(path: &std::path::Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let length = size.min(TAIL_BYTES);
    file.seek(SeekFrom::Start(size - length)).ok()?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).trim_end().rsplit('\n').next()?.to_owned())
}

fn skipped(cgroup: &str) -> bool {
    SKIPPED.iter().any(|name| cgroup == *name || cgroup.strip_prefix(name).is_some_and(|rest| rest.starts_with('/')))
}

fn round(value: f64) -> f64 {
    (value * 100.0 + 0.5).floor() / 100.0
}

fn pick(item: &Value, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|key| Some(((*key).to_owned(), item.get(*key)?.clone()))).collect::<Map<_, _>>())
}

fn provider(item: &Value) -> bool {
    let command = item["command"].as_str().unwrap_or_default();
    command.split(' ').next().and_then(|program| program.rsplit('/').next()).is_some_and(|name| SESSION_PROVIDERS.contains(&name))
}

fn session(item: &Value) -> Value {
    let command = item["command"].as_str().unwrap_or_default();
    let arguments: Vec<&str> = command.split(' ').collect();
    let id = arguments.windows(2).find(|pair| SESSION_FLAGS.contains(&pair[0]) && session_id(pair[1])).map(|pair| pair[1]);
    json!({ "pid": item["pid"], "session": id, "cores": item["cores"], "command": item["command"] })
}

fn session_id(text: &str) -> bool {
    (32..=36).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-')
}
