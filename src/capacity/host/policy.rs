use std::sync::LazyLock;

use serde_json::{Value, json};
use toml_edit::DocumentMut;

use super::json::num;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resources {
    pub(crate) cpus: f64,
    pub(crate) memory_mib: f64,
}

impl Resources {
    pub(crate) fn json(self) -> Value {
        json!({ "cpus": num(self.cpus), "memoryMiB": num(self.memory_mib) })
    }
}

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

fn default_number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(crate) const CLASS_NAMES: [&str; 8] = ["agent", "native", "moderate", "heavy", "exclusive", "gpu", "critical", "tom"];
pub(crate) const GPU_BUSY_LIMIT_PERCENT: f64 = 85.0;
const LEARNED_MINIMUM_RUNS: usize = 3;

pub(crate) fn reserved_cpus(profile: &str, cores: f64) -> f64 {
    match profile {
        "attended" if cores > 8.0 => 4.0,
        "attended" if cores > 4.0 => 2.0,
        "attended" => 1.0,
        _ => 0.0,
    }
}

pub(crate) fn aggregate_cpus(profile: &str, cores: f64) -> f64 {
    cores - reserved_cpus(profile, cores)
}

pub(crate) fn grantable_maximum(profile: &str, cores: f64, memory_total_mib: f64) -> Resources {
    Resources { cpus: aggregate_cpus(profile, cores), memory_mib: (memory_total_mib * 3.0 / 4.0).floor() }
}

/// The request cut to what may be granted, with each cut field as `[requested, granted]`.
pub(crate) fn clamp_to_grantable(request: Resources, maximum: Resources) -> (Resources, Option<Value>) {
    let granted = Resources { cpus: request.cpus.min(maximum.cpus), memory_mib: request.memory_mib.min(maximum.memory_mib) };
    let mut clamped = serde_json::Map::new();
    if granted.cpus != request.cpus {
        clamped.insert("cpus".to_owned(), json!([num(request.cpus), num(granted.cpus)]));
    }
    if granted.memory_mib != request.memory_mib {
        clamped.insert("memoryMiB".to_owned(), json!([num(request.memory_mib), num(granted.memory_mib)]));
    }
    (granted, (!clamped.is_empty()).then_some(Value::Object(clamped)))
}

pub(crate) fn resource_class(kind: &str, cores: f64) -> Option<Resources> {
    let pair = if cores > 1.0 { 2.0 } else { 1.0 };
    let (cpus, memory_mib) = match kind {
        "agent" => (0.0, 768.0),
        "native" => (pair, 4096.0),
        "moderate" | "tom" => (pair, 2048.0),
        "heavy" if cores < 8.0 => (cores, 8192.0),
        "heavy" => (if cores > 8.0 { 6.0 } else { 3.0 }, 8192.0),
        "gpu" => (1.0, 2048.0),
        "critical" if cores < 8.0 => (cores, 16384.0),
        "critical" => (cores - reserved_cpus("attended", cores), 16384.0),
        "exclusive" => (cores, 16384.0),
        _ => return None,
    };
    Some(Resources { cpus, memory_mib })
}

pub(crate) fn native_memory_request(kind: &str, cores: f64, memory_gib: f64) -> Result<Resources, &'static str> {
    let base = resource_class(kind, cores).ok_or("Unknown resource class")?;
    if kind != "native" || memory_gib.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Err("--memory-gib requires a positive native memory request");
    }
    Ok(Resources { cpus: base.cpus, memory_mib: memory_gib * 1024.0 })
}

pub(crate) fn batch_class(kind: &str) -> bool {
    matches!(kind, "moderate" | "heavy" | "exclusive" | "gpu" | "critical" | "tom")
}

pub(crate) fn maximum_seconds(kind: &str) -> u64 {
    if kind == "exclusive" { 900 } else { 3600 }
}

pub(crate) fn session_seconds(kind: &str) -> u64 {
    match kind {
        "exclusive" => 900,
        kind if batch_class(kind) => 1800,
        _ => 0,
    }
}

/// When the GPU first crossed the busy limit and how long it has stayed there.
pub(crate) fn gpu_busy_observation(now: f64, busy_percent: f64, since: Option<f64>) -> (Option<f64>, f64) {
    let since = (busy_percent >= GPU_BUSY_LIMIT_PERCENT).then(|| since.unwrap_or(now));
    (since, since.map_or(0.0, |since| (now - since).max(0.0) / 1000.0))
}

pub(crate) fn sized_class(gpu_share: f64, cpus: f64, memory_mib: f64) -> &'static str {
    if gpu_share >= 0.2 {
        "gpu"
    } else if cpus <= 2.0 && memory_mib <= 2048.0 {
        "moderate"
    } else {
        "heavy"
    }
}

/// The smallest value at or above which nine tenths of the values fall, or -1 for none.
pub(crate) fn p90(values: &[f64]) -> f64 {
    let mut best = -1.0;
    for &value in values {
        let below = values.iter().filter(|&&other| other <= value).count();
        if below * 10 >= values.len() * 9 && (best < 0.0 || value < best) {
            best = value;
        }
    }
    best
}

pub(crate) fn learned_minimum_runs() -> usize {
    LEARNED_MINIMUM_RUNS
}

pub(crate) fn memory_margin() -> f64 {
    default_number("capacity_memory_margin")
}

pub(crate) fn sizing_runs() -> usize {
    default_number("capacity_sizing_runs") as usize
}

pub(crate) fn quiet_idle_cores() -> f64 {
    default_number("quiet_idle_cores")
}

pub(crate) fn high_confidence_runs() -> usize {
    default_number("capacity_high_confidence_runs") as usize
}

/// What a declared class is charged once enough of its own runs measured it.
pub(crate) fn charged_resources(kind: &str, cores: f64, run_cores: &[f64], run_memory_mib: &[f64]) -> Option<Resources> {
    let declared = resource_class(kind, cores)?;
    let learned = matches!(kind, "moderate" | "heavy" | "native" | "gpu");
    if !learned || run_cores.len() < LEARNED_MINIMUM_RUNS || run_memory_mib.len() < LEARNED_MINIMUM_RUNS {
        return Some(declared);
    }
    let cpus = p90(run_cores).min(declared.cpus * 1.5).max(1.0);
    Some(Resources { cpus: if kind == "heavy" { cpus.max(declared.cpus) } else { cpus }, memory_mib: p90(run_memory_mib) * memory_margin() })
}

pub(crate) fn owner_prefix(owner: &str) -> &str {
    let end = owner.find(|character: char| !character.is_ascii_alphabetic()).unwrap_or(owner.len());
    &owner[..end]
}

pub(crate) fn disk_busy_share(before: &[(String, f64)], after: &[(String, f64)], wall_milliseconds: f64) -> Option<f64> {
    let deltas: Vec<f64> = after
        .iter()
        .filter_map(|(disk, ticks)| before.iter().find(|(name, _)| name == disk).filter(|(_, earlier)| ticks >= earlier).map(|(_, earlier)| ticks - earlier))
        .collect();
    (wall_milliseconds > 0.0 && !deltas.is_empty()).then(|| (deltas.iter().copied().fold(f64::MIN, f64::max) / wall_milliseconds).min(1.0))
}

pub(crate) fn scope_cpu_quota(kind: &str, charged_cpus: f64, declared_cpus: Option<f64>) -> Option<f64> {
    (kind != "native").then(|| declared_cpus.unwrap_or(charged_cpus))
}

/// The CPUs a running lease holds against the aggregate, and which evidence set it.
pub(crate) fn running_cpu_commitment(kind: &str, measured: Option<f64>, estimate_p50: Option<f64>, reserved: f64, quota: Option<f64>) -> (f64, &'static str) {
    let measured = measured.filter(|value| value.is_finite() && *value >= 0.0);
    let predicted = estimate_p50.filter(|value| value.is_finite() && *value >= 0.0).unwrap_or(reserved);
    let cpus = match (kind, measured) {
        ("native", Some(measured)) => measured * 1.25,
        ("heavy", _) => (measured.unwrap_or(predicted) * 1.25).max(reserved),
        _ => measured.unwrap_or(0.0).max(predicted) * 1.25,
    };
    match quota {
        Some(quota) if cpus > quota => (quota, "scope-quota"),
        _ if measured.is_none() => (cpus, "estimate-missing-measurement"),
        _ if kind == "native" => (cpus, "native-measured"),
        _ => (cpus, "measured-or-estimate"),
    }
}

/// `percentile` over the values: the ceil(share × n)-th smallest.
pub(crate) fn percentile(values: &[f64], share: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((share * sorted.len() as f64).ceil() as usize).saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}
