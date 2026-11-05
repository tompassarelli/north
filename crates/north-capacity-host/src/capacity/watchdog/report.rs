use north_base::foundation::clock::Timestamp;
use serde_json::{Value, json};

use super::sampling::round2;

const PSI_LIMIT: f64 = 50.0;
const CLIENT_SHARE: f64 = 1.6;
const OVERSHOOT: f64 = 1.1;
const MEMORY_FLOOR_MIB: f64 = 8192.0;
const MEMORY_FLOOR_SHARE: f64 = 0.2;
const FLOOR_WARNING_MINUTES: f64 = 30.0;
const TOP_UNLEASED: usize = 5;
const CLAUDE_TIERS: [&str; 7] = ["haiku", "sonnet", "low", "medium", "high", "xhigh", "max"];

pub(super) const HANDOFF_FIX: &str = "2026-10-10T20:24:00Z";

pub(super) struct Run {
    pub(super) agent: String,
    pub(super) tier: String,
    pub(super) role: String,
    pub(super) ended: String,
    pub(super) peak_ctx: Option<f64>,
    pub(super) handoff_fires: Option<i64>,
    pub(super) handoff_ignored: Option<i64>,
}

pub(super) struct Inputs<'a> {
    pub(super) samples: &'a [Value],
    pub(super) incidents: &'a [String],
    pub(super) runs: &'a [Run],
    pub(super) ledger_since: Option<i64>,
    pub(super) handoff_ctx: f64,
    pub(super) now: i64,
}

pub(super) fn parse_time(text: &str) -> Option<i64> {
    Timestamp::parse_rfc3339(text).or_else(|| Timestamp::parse_loose(text)).map(Timestamp::unix_millis)
}

pub(super) fn handoff_fix() -> i64 {
    parse_time(HANDOFF_FIX).expect("the handoff fix instant parses")
}

fn round_half_up(number: f64) -> f64 {
    (number + 0.5).floor()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
}

fn mean(values: &[f64], count: usize) -> f64 {
    values.iter().sum::<f64>() / count as f64
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

struct Owner {
    name: String,
    class: String,
    cpus: f64,
    seen: Vec<f64>,
    org: Option<String>,
}

fn handoff_flags(inputs: &Inputs, since: i64) -> Vec<String> {
    let earliest = inputs.ledger_since.unwrap_or_else(|| since.max(handoff_fix()));
    let mut flags = Vec::new();
    for run in inputs.runs {
        let in_window = parse_time(&run.ended).is_some_and(|ended| ended >= earliest && ended <= inputs.now);
        if !in_window || run.role != "worker" || !CLAUDE_TIERS.contains(&run.tier.as_str()) {
            continue;
        }
        if run.peak_ctx.unwrap_or(0.0) > inputs.handoff_ctx && run.handoff_fires == Some(0) {
            flags.push(format!("handoff hook silent: {} peak {}k, 0 fires", run.agent, round_half_up(run.peak_ctx.unwrap_or(0.0) / 1000.0)));
        }
        if run.handoff_fires.unwrap_or(0) > 0 && run.handoff_ignored.unwrap_or(0) > 0 {
            flags.push(format!("handoff ignored: {}", run.agent));
        }
    }
    flags
}

fn lease_owners(window: &[&Value]) -> Vec<Owner> {
    let mut owners: Vec<Owner> = Vec::new();
    for sample in window {
        for lease in sample["leases"].as_array().into_iter().flatten() {
            let cpus = number(&lease["cpus"]).max(lease["declaredCpus"].as_f64().unwrap_or(number(&lease["cpus"])));
            let (name, class) = (lease["owner"].as_str().unwrap_or_default(), lease["class"].as_str().unwrap_or_default());
            let at = match owners.iter().position(|o| o.name == name && o.class == class && o.cpus == cpus) {
                Some(at) => at,
                None => {
                    owners.push(Owner { name: name.to_owned(), class: class.to_owned(), cpus, seen: Vec::new(), org: None });
                    owners.len() - 1
                }
            };
            owners[at].seen.push(lease["recentCores"].as_f64().unwrap_or(0.0));
            owners[at].org = lease["org"].as_str().map(str::to_owned).or(owners[at].org.take());
        }
    }
    owners
}

fn client_flags(window: &[&Value]) -> Vec<String> {
    let mut clients: Vec<(String, Vec<f64>)> = Vec::new();
    for sample in window {
        for client in sample["clients"].as_array().into_iter().flatten() {
            let key = format!("{}#{}", client["comm"].as_str().unwrap_or_default(), client["pid"]);
            match clients.iter_mut().find(|(name, _)| *name == key) {
                Some((_, seen)) => seen.push(number(&client["cores"])),
                None => clients.push((key, vec![number(&client["cores"])])),
            }
        }
    }
    clients
        .iter()
        .filter(|(_, seen)| seen.len() >= 2 && median(seen) > CLIENT_SHARE)
        .map(|(client, seen)| format!("client {client} median {} cores over its measured share {CLIENT_SHARE}", round2(median(seen))))
        .collect()
}

fn memory_flags(window: &[&Value]) -> (Vec<String>, f64, f64) {
    let (first, last) = (window[0], window[window.len() - 1]);
    let floor = MEMORY_FLOOR_MIB.max(round_half_up(number(&last["memTotalMiB"]) * MEMORY_FLOOR_SHARE));
    let span = (parse_time(last["at"].as_str().unwrap_or_default()).unwrap_or(0) - parse_time(first["at"].as_str().unwrap_or_default()).unwrap_or(0)) as f64 / 60000.0;
    let available = number(&last["memAvailableMiB"]);
    let slope = if span > 0.0 { (available - number(&first["memAvailableMiB"])) / span } else { 0.0 };
    let to_floor = (slope < 0.0).then(|| (available - floor) / -slope);
    let mut flags = Vec::new();
    if available < floor {
        flags.push(format!("memory available {available} MiB is under the {floor} MiB floor"));
    } else if let Some(minutes) = to_floor.filter(|minutes| *minutes < FLOOR_WARNING_MINUTES) {
        flags.push(format!("memory available falling {} MiB/min, reaches the {floor} MiB floor in {} min", round_half_up(-slope), round_half_up(minutes)));
    }
    (flags, slope, floor)
}

pub(super) fn report(inputs: &Inputs, minutes: f64) -> Value {
    let since = inputs.now - (minutes * 60000.0) as i64;
    let window: Vec<&Value> = inputs.samples.iter().filter(|sample| sample["at"].as_str().and_then(parse_time).is_some_and(|at| at >= since)).collect();
    let incidents: Vec<&String> = inputs.incidents.iter().filter(|line| line.strip_prefix("- ").and_then(|rest| rest.split_whitespace().next()).and_then(parse_time).is_some_and(|at| at >= since)).collect();
    let mut flags = handoff_flags(inputs, since);
    if window.is_empty() {
        flags.insert(0, "no samples: is capacity-watchdog.service running?".to_owned());
        return json!({ "minutes": minutes, "samples": 0, "incidents": incidents, "flags": flags });
    }
    let psi: Vec<f64> = window.iter().map(|sample| number(&sample["cpuPsi"])).collect();
    let high = psi.iter().filter(|value| **value > PSI_LIMIT).count();
    let peak = psi.iter().copied().fold(f64::MIN, f64::max);
    if high as f64 >= 2f64.max(window.len() as f64 / 2.0) {
        flags.push(format!("cpu PSI above {PSI_LIMIT}% in {high}/{} samples (max {peak})", window.len()));
    }
    let owners = lease_owners(&window);
    for owner in &owners {
        let middle = median(&owner.seen);
        if owner.class != "native" && owner.seen.len() >= 2 && middle > owner.cpus * OVERSHOOT {
            let org = owner.org.as_ref().map(|org| format!(" org={org}")).unwrap_or_default();
            flags.push(format!("{} lease {} median {} cores over its {} declared{org}", owner.class, owner.name, round2(middle), owner.cpus));
        }
    }
    flags.extend(client_flags(&window));
    let (memory, slope, floor) = memory_flags(&window);
    flags.extend(memory);
    let last = window[window.len() - 1];
    let mut unleased: Vec<(String, Vec<f64>)> = Vec::new();
    for sample in &window {
        for top in sample["top"].as_array().into_iter().flatten() {
            let cgroup = top["cgroup"].as_str().unwrap_or_default();
            match unleased.iter_mut().find(|(name, _)| name == cgroup) {
                Some((_, seen)) => seen.push(number(&top["cores"])),
                None => unleased.push((cgroup.to_owned(), vec![number(&top["cores"])])),
            }
        }
    }
    let mut top_unleased: Vec<Value> = unleased.iter().map(|(cgroup, seen)| json!({ "cgroup": cgroup, "meanCores": round2(mean(seen, window.len())) })).collect();
    top_unleased.sort_by(|left, right| number(&right["meanCores"]).total_cmp(&number(&left["meanCores"])));
    top_unleased.truncate(TOP_UNLEASED);
    if !incidents.is_empty() {
        flags.push(format!("{} incident(s) in window", incidents.len()));
    }
    let least = window.iter().map(|sample| number(&sample["memAvailableMiB"])).fold(f64::MAX, f64::min);
    json!({
        "minutes": minutes, "samples": window.len(), "load": last["load"],
        "cpuPsi": { "mean": round2(mean(&psi, psi.len())), "max": peak, "last": last["cpuPsi"] },
        "memAvailableMiB": { "last": last["memAvailableMiB"], "min": least, "slopePerMin": round_half_up(slope), "floor": floor },
        "topUnleased": top_unleased,
        "leases": owners.iter().map(|o| json!({ "owner": o.name, "class": o.class, "cpus": o.cpus, "medianCores": round2(median(&o.seen)), "org": o.org })).collect::<Vec<_>>(),
        "incidents": incidents, "flags": flags,
    })
}
