//! The idle-capacity and pin guards beside the stall verdict: each reads one source and says what
//! it saw (`text`) and, when it is wrong, why (`reason`).
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::host::{Host, seconds};

pub(crate) const ACCOUNT_JOBS: u64 = 40;
const IDLE_JOBS: u64 = 10;
const IDLE_PENDING: u64 = 5;
pub(crate) const IDLE_SECONDS: f64 = 900.0;
pub(crate) const CACHE_SECONDS: f64 = 300.0;
const REUSE_WINDOW: usize = 20;
const NATIVE_LABELS: &str = "label:needs:native-pair,needs:native-single";
const WISP_STATUS: [&str; 4] = ["bun", "wisp", "lan", "status"];

static CARGO_PIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"git\s*=\s*"https://github\.com/([\w.-]+/[\w.-]+?)(?:\.git)?"[^}\n]*?\btag\s*=\s*"([^"]+)""#).unwrap()
});

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Guard {
    pub(crate) text: String,
    pub(crate) reason: Option<String>,
}

impl Guard {
    fn new(text: impl Into<String>, reason: Option<String>) -> Self {
        Self { text: text.into(), reason }
    }
}

pub(crate) fn reuse_verdict(flags: Option<&[Option<bool>]>) -> Guard {
    let unknown = Guard::new("reuse n/a", None);
    let Some(flags) = flags else { return unknown };
    let recent = &flags[..flags.len().min(REUSE_WINDOW)];
    if recent.is_empty() || recent.iter().any(Option::is_none) {
        return unknown;
    }
    let reused = recent.iter().filter(|flag| **flag == Some(true)).count();
    let text = format!("reuse {reused}/{}", recent.len());
    let reason = (reused * 2 < recent.len()).then(|| format!("census {text} below 50%"));
    Guard::new(text, reason)
}

pub(crate) fn cargo_pins(manifests: &[String]) -> Vec<(String, String)> {
    let found: BTreeSet<(String, String)> = manifests
        .iter()
        .flat_map(|text| CARGO_PIN.captures_iter(text))
        .map(|captures| (captures[1].to_owned(), captures[2].to_owned()))
        .collect();
    found.into_iter().collect()
}

pub(crate) struct Build {
    pub(crate) dependency: String,
    pub(crate) tag: String,
    pub(crate) status: String,
    pub(crate) conclusion: String,
}

pub(crate) fn pins_verdict(builds: Option<&[Build]>) -> Guard {
    let Some(builds) = builds else { return Guard::new("pins n/a", None) };
    let red: Vec<String> = builds
        .iter()
        .filter(|build| build.status == "completed" && build.conclusion != "success")
        .map(|build| format!("{}@{} built {}", build.dependency, build.tag, build.conclusion))
        .collect();
    Guard::new(
        format!("pins {}/{} green", builds.len() - red.len(), builds.len()),
        (!red.is_empty()).then(|| format!("pinned tag build not green: {}", red.join(", "))),
    )
}

pub(crate) fn pin_builds(host: &dyn Host, checkout: &Path) -> Option<Vec<Build>> {
    let manifests: Vec<String> = ["Cargo.toml", "src-tauri/Cargo.toml"]
        .iter()
        .filter_map(|name| host.run(&["git", "show", &format!("origin/main:{name}")], Some(checkout), None))
        .collect();
    cargo_pins(&manifests)
        .into_iter()
        .map(|(dependency, tag)| {
            let listed = host.json(&["gh", "run", "list", "--repo", &dependency, "--branch", &tag, "--limit", "1", "--json", "status,conclusion"])?;
            let last = listed.get(0)?;
            Some(Build {
                status: last["status"].as_str().unwrap_or_default().into(),
                conclusion: last["conclusion"].as_str().unwrap_or_default().into(),
                dependency,
                tag,
            })
        })
        .collect()
}

pub(crate) fn actions_verdict(pending: Option<u64>, samples: &[(f64, u64)], now: f64) -> Guard {
    let recent = now - IDLE_SECONDS - CACHE_SECONDS;
    let window: Vec<u64> = samples.iter().filter(|(at, _)| (recent..=now).contains(at)).map(|(_, busy)| *busy).collect();
    let covered = samples.iter().any(|(at, _)| (recent..=now - IDLE_SECONDS).contains(at));
    let current = samples.iter().max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, busy)| *busy);
    let text = format!("actions {}/{ACCOUNT_JOBS} busy", current.map_or("?".into(), |busy| busy.to_string()));
    let peak = window.iter().copied().max().unwrap_or(0);
    let flagged = pending.is_some_and(|pending| pending >= IDLE_PENDING) && covered && peak < IDLE_JOBS;
    let reason = flagged.then(|| {
        format!("idle Actions slots: at most {peak}/{ACCOUNT_JOBS} jobs busy for {}m with {} pending", IDLE_SECONDS as u64 / 60, pending.unwrap_or(0))
    });
    Guard::new(text, reason)
}

fn epoch(value: &Value) -> Option<f64> {
    match value.as_f64() {
        Some(number) if number > 1e12 => Some(number / 1000.0),
        Some(number) => Some(number),
        None => seconds(value.as_str()?),
    }
}

fn idle_pairs(pairs: &[Value], now: f64) -> Vec<(String, f64)> {
    pairs
        .iter()
        .filter_map(|pair| {
            let seconds = if let Some(state) = pair["state"].as_str() {
                if state == "idle" { pair["idleSeconds"].as_f64().unwrap_or(0.0) } else { 0.0 }
            } else {
                (now - epoch(&pair["driver"]["goneAt"])?).max(0.0)
            };
            (seconds > IDLE_SECONDS).then(|| (pair["pair"].to_string().trim_matches('"').to_owned(), seconds))
        })
        .collect()
}

/// `waiting` is the count of native-check issues, `None` when the count was unreadable.
pub(crate) fn wisp_verdict(pairs: Option<&[Value]>, waiting: Option<u64>, now: f64) -> Guard {
    let Some(pairs) = pairs else { return Guard::new("wisp no pool", None) };
    let idle = idle_pairs(pairs, now);
    let dead: Vec<(String, Vec<String>)> = pairs
        .iter()
        .filter(|pair| pair["state"].as_str() == Some("dead"))
        .map(|pair| {
            let down = pair["down"].as_array().map(|down| down.iter().filter_map(|name| name.as_str().map(str::to_owned)).collect());
            (pair["pair"].to_string().trim_matches('"').to_owned(), down.unwrap_or_default())
        })
        .collect();
    let shown = waiting.map_or("error".into(), |count| count.to_string());
    let text = format!(
        "wisp {}/{} pairs idle >{}m{}, native-check waiting {shown}",
        idle.len(),
        pairs.len(),
        IDLE_SECONDS as u64 / 60,
        if dead.is_empty() { String::new() } else { format!(", {} dead", dead.len()) }
    );
    let flagged = (!idle.is_empty() || !dead.is_empty()) && waiting.is_some_and(|count| count >= 1);
    let parts: Vec<String> = idle
        .iter()
        .map(|(name, seconds)| format!("pair {name} idle {:.0}m", seconds / 60.0))
        .chain(dead.iter().map(|(name, down)| {
            format!("pair {name} dead: {} down", if down.is_empty() { "a client".into() } else { down.join(" ") })
        }))
        .collect();
    Guard::new(text, flagged.then(|| format!("idle Wisp pool: {} with {shown} native-check waiting", parts.join(", "))))
}

enum Pool {
    Absent,
    Error,
    Pairs(Vec<Value>),
}

fn wisp_pool(host: &dyn Host, home: &Path) -> Pool {
    let tree = home.join("code/smashcraft/main/ts");
    let Some(output) = host.output(&WISP_STATUS, Some(&tree), None, Duration::from_secs(40)) else { return Pool::Error };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let said = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
        return if said.contains("no pool is running") { Pool::Absent } else { Pool::Error };
    }
    let pairs = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|item| item.as_object().is_some_and(|object| object.contains_key("pair")))
        .collect();
    Pool::Pairs(pairs)
}

fn native_waiting(host: &dyn Host, slug: &str) -> Option<u64> {
    let listed = host.json(&["gh", "issue", "list", "--repo", slug, "--state", "open", "--limit", "100", "--search", NATIVE_LABELS, "--json", "number"])?;
    Some(listed.as_array()?.len() as u64)
}

pub(crate) fn wisp_row(host: &dyn Host, home: &Path, slug: &str, now: f64) -> Guard {
    match wisp_pool(host, home) {
        Pool::Error => Guard::new("wisp error", None),
        Pool::Absent => wisp_verdict(None, Some(0), now),
        Pool::Pairs(pairs) if pairs.is_empty() => wisp_verdict(Some(&pairs), Some(1), now),
        Pool::Pairs(pairs) => {
            let probe = wisp_verdict(Some(&pairs), Some(1), now);
            wisp_verdict(Some(&pairs), if probe.reason.is_some() { native_waiting(host, slug) } else { Some(0) }, now)
        }
    }
}
