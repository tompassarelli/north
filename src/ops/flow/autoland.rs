//! What GitHub and the safe-push log say about lanes: when each was pushed and landed, which have
//! failed, and how long the trip took.
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::host::{Host, Places, iso, median, seconds};

const DAY: f64 = 86_400.0;
const CENSUS_REUSE_FIELD: &str = "census-reused";

static LANDED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^landed as [0-9a-f]{40,64}(?:\s|$)").unwrap());
static PUSH_SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40,64}$").unwrap());
static REUSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\b{CENSUS_REUSE_FIELD}[=:] ?(yes|no|true|false)\b")).unwrap());

pub(crate) struct Latency {
    pub(crate) median: Value,
    pub(crate) p90: Value,
    pub(crate) reuse: Option<Vec<Option<bool>>>,
    pub(crate) pushes: Option<Vec<f64>>,
    pub(crate) landed: Option<Vec<f64>>,
}

pub(crate) fn percentiles(samples: &[f64]) -> (Option<f64>, Option<f64>) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    match sorted.len() {
        0 => (None, None),
        length => (median(samples), Some(sorted[(length as f64 * 0.9).ceil() as usize - 1])),
    }
}

fn number(value: Option<f64>) -> Value {
    value.map_or(Value::Null, Value::from)
}

fn contexts(tip: &Value) -> impl Iterator<Item = &Value> {
    tip["status"]["contexts"].as_array().into_iter().flatten()
}

fn landed_status(status: &Value) -> bool {
    status["context"] == "autoland" && status["state"] == "SUCCESS" && LANDED.is_match(status["description"].as_str().unwrap_or_default())
}

pub(crate) fn landed_times(tips: &BTreeMap<String, Value>) -> Vec<f64> {
    tips.values().filter_map(|tip| contexts(tip).find(|status| landed_status(status)).and_then(|status| seconds(status["createdAt"].as_str()?))).collect()
}

fn first_pushes<'a>(pushes: impl IntoIterator<Item = &'a Value>, since: Option<(f64, f64)>) -> BTreeMap<String, f64> {
    let mut first = BTreeMap::new();
    for push in pushes {
        let (Some(branch), Some(sha), Some(at)) = (push["headBranch"].as_str(), push["headSha"].as_str(), push["createdAt"].as_str().and_then(seconds)) else { continue };
        if branch.starts_with("claude/land-") && since.is_none_or(|(from, to)| (from..=to).contains(&at)) {
            let seen = first.entry(sha.to_owned()).or_insert(at);
            *seen = seen.min(at);
        }
    }
    first
}

pub(crate) fn autoland_samples(pushes: &[Value], tips: &BTreeMap<String, Value>, now: f64) -> (Vec<f64>, Vec<Option<bool>>) {
    let pushed = first_pushes(pushes, Some((now - DAY, now)));
    let mut samples = Vec::new();
    let mut reuse = Vec::new();
    for tip in tips.values().filter(|tip| !tip.is_null()) {
        let Some(&at) = tip["oid"].as_str().and_then(|oid| pushed.get(oid)) else { continue };
        if tip["status"].is_null() {
            continue;
        }
        for status in contexts(tip).filter(|status| status["context"] == "autoland" && status["state"] == "SUCCESS") {
            let description = status["description"].as_str().unwrap_or_default();
            if !LANDED.is_match(description) {
                continue;
            }
            let Some(landed) = status["createdAt"].as_str().and_then(seconds) else { continue };
            if at <= landed && landed <= now {
                samples.push((landed - at) / 60.0);
                let flag = REUSE.captures(description).map(|found| matches!(&found[1], "yes" | "true"));
                reuse.push((landed, flag));
            }
            break;
        }
    }
    reuse.sort_by(|a, b| b.0.total_cmp(&a.0));
    (samples, reuse.into_iter().map(|(_, flag)| flag).collect())
}

fn tip_query(shas: &[String], fields: &str, owner: &str, name: &str) -> String {
    let objects: Vec<String> = shas
        .iter()
        .enumerate()
        .map(|(index, sha)| format!("tip{index}: object(oid:{sha:?}) {{ ... on Commit {{ oid status {{ contexts {{ {fields} }} }} }} }}"))
        .collect();
    format!("{{ repository(owner:{owner:?}, name:{name:?}) {{ {} }} }}", objects.join(" "))
}

fn tips_of(response: &Value) -> Option<BTreeMap<String, Value>> {
    Some(response["data"]["repository"].as_object()?.iter().map(|(key, tip)| (key.clone(), tip.clone())).collect())
}

fn land_push_times(pushes: &[Value]) -> Vec<f64> {
    first_pushes(pushes, None).into_values().collect()
}

pub(crate) fn autoland_latency(host: &dyn Host, slug: &str, now: f64) -> Latency {
    let failed = |pushes: Option<Vec<f64>>| Latency { median: "error".into(), p90: "error".into(), reuse: None, pushes, landed: None };
    let since = iso(now - DAY);
    let listed = host.json(&["gh", "run", "list", "--repo", slug, "--workflow", "autoland.yml", "--event", "push", "--created", &format!(">={since}"), "--limit", "1000", "--json", "headBranch,headSha,createdAt"]);
    let Some(pushes) = listed.as_ref().and_then(Value::as_array) else { return failed(None) };
    let mut shas: Vec<String> = Vec::new();
    for push in pushes.iter().filter(|push| push["headBranch"].as_str().is_some_and(|branch| branch.starts_with("claude/land-"))) {
        let sha = push["headSha"].as_str().unwrap_or_default().to_owned();
        if !shas.contains(&sha) {
            shas.push(sha);
        }
    }
    if shas.is_empty() {
        return Latency { median: Value::Null, p90: Value::Null, reuse: Some(Vec::new()), pushes: Some(Vec::new()), landed: Some(Vec::new()) };
    }
    let (owner, name) = slug.split_once('/').unwrap_or((slug, ""));
    let response = host.graphql(&tip_query(&shas, "context state description createdAt", owner, name));
    let Some(response) = response else { return failed(None) };
    if response.get("errors").is_some_and(|errors| !errors.is_null() && errors != &Value::Array(Vec::new())) {
        return failed(Some(land_push_times(pushes)));
    }
    let Some(tips) = tips_of(&response) else { return failed(None) };
    let (samples, reuse) = autoland_samples(pushes, &tips, now);
    let (middle, p90) = percentiles(&samples);
    Latency { median: number(middle), p90: number(p90), reuse: Some(reuse), pushes: Some(land_push_times(pushes)), landed: Some(landed_times(&tips)) }
}

pub(crate) fn autoland_states(host: &dyn Host, slug: &str, shas: &[String]) -> BTreeMap<String, Value> {
    if shas.is_empty() {
        return BTreeMap::new();
    }
    let (owner, name) = slug.split_once('/').unwrap_or((slug, ""));
    let Some(tips) = host.graphql(&tip_query(shas, "context state createdAt", owner, name)).as_ref().and_then(tips_of) else { return BTreeMap::new() };
    tips.values()
        .filter(|tip| !tip.is_null())
        .filter_map(|tip| Some((tip["oid"].as_str()?.to_owned(), contexts(tip).filter(|status| status["context"] == "autoland").last()?.clone())))
        .collect()
}

pub(crate) fn failed_since(states: &BTreeMap<String, Value>, sha: &str) -> Option<f64> {
    let state = states.get(sha)?;
    matches!(state["state"].as_str()?, "FAILURE" | "ERROR").then(|| seconds(state["createdAt"].as_str()?)).flatten()
}

/// Pending, dead and the age in minutes of the oldest dead lane.
pub(crate) fn lane_counts(shas: &[String], states: &BTreeMap<String, Value>, now: f64) -> (usize, usize, Option<f64>) {
    let dead: Vec<f64> = shas.iter().filter_map(|sha| failed_since(states, sha)).collect();
    let oldest = dead.iter().copied().min_by(f64::total_cmp).map(|at| (now - at).max(0.0) / 60.0);
    (shas.len() - dead.len(), dead.len(), oldest)
}

fn patch_id(host: &dyn Host, checkout: &Path, sha: &str) -> Option<String> {
    let patch = host.run(&["git", "show", "--pretty=format:", "--no-ext-diff", sha], Some(checkout), None).filter(|patch| !patch.is_empty())?;
    host.run(&["git", "patch-id", "--stable"], Some(checkout), Some(&patch))?.split_whitespace().next().map(str::to_owned)
}

fn safe_push_log(places: &Places, slug: &str) -> Result<BTreeMap<String, f64>, std::io::ErrorKind> {
    let text = std::fs::read_to_string(places.state.join("safe-push/pushes.tsv")).map_err(|error| error.kind())?;
    let project = slug.rsplit('/').next().unwrap_or(slug);
    let mut first: BTreeMap<String, f64> = BTreeMap::new();
    for fields in text.lines().map(|line| line.split('\t').collect::<Vec<_>>()) {
        if fields.len() < 4 || fields[1] != project {
            continue;
        }
        let Ok(at) = fields[0].parse::<f64>() else { continue };
        let seen = first.entry(fields[3].to_owned()).or_insert(at);
        *seen = seen.min(at);
    }
    Ok(first)
}

pub(crate) fn local_push_times(places: &Places, slug: &str) -> Vec<f64> {
    safe_push_log(places, slug).map(|pushes| pushes.into_values().collect()).unwrap_or_default()
}

/// Push-to-land median and p90 minutes for lanes landed straight to main, read from safe-push's log.
pub(crate) fn local_latency(host: &dyn Host, places: &Places, checkout: &Path, slug: &str, commits: &[(String, i64)]) -> (Value, Value) {
    let pushes = match safe_push_log(places, slug) {
        Ok(pushes) => pushes,
        Err(std::io::ErrorKind::NotFound) => return (Value::Null, Value::Null),
        Err(_) => return ("error".into(), "error".into()),
    };
    let mut patches: Option<BTreeMap<String, f64>> = None;
    let mut samples = Vec::new();
    for (sha, pushed) in pushes.iter().filter(|(sha, _)| PUSH_SHA.is_match(sha)) {
        let direct = commits.iter().find(|(head, _)| head == sha).map(|(_, at)| *at as f64);
        let landed = direct.or_else(|| {
            let pid = patch_id(host, checkout, sha)?;
            let by_patch = patches.get_or_insert_with(|| {
                commits.iter().rev().filter_map(|(head, at)| Some((patch_id(host, checkout, head)?, *at as f64))).collect()
            });
            by_patch.get(&pid).copied()
        });
        samples.extend(landed.map(|landed| (landed - pushed).max(0.0) / 60.0));
    }
    let (middle, p90) = percentiles(&samples);
    (number(middle), number(p90))
}
