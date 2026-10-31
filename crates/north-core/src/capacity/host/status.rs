use std::path::Path;

use serde_json::{Value, json};

use super::decision::gpu_view;
use super::json::{Obj, field, num, round2, text};
use super::machine::{now_ms, read_gpu, read_signals};
use super::policy::{aggregate_cpus, batch_class};
use super::request::{class, cpu_accounting, id, is_run, lease_facts};
use super::state::{active_profile, priority_order, read_leases, read_queue, read_usage, record_gpu_busy, with_lock};

fn seconds(milliseconds: f64) -> Value {
    num((milliseconds / 1000.0 + 0.5).floor())
}

fn measured_view(usage: Option<Value>) -> Value {
    let Some(usage) = usage else { return Value::Null };
    let rounded = |key: &str| field(&usage, key).map_or(Value::Null, |value| num(round2(value)));
    let whole = |key: &str| field(&usage, key).map_or(Value::Null, |value| num((value + 0.5).floor()));
    json!({
        "cores": rounded("recentCores"), "liveCores": rounded("liveCores"), "liveWindowSeconds": usage.get("liveWindowSeconds").cloned().unwrap_or(Value::Null),
        "peakCores": rounded("peakCores"), "meanCores": rounded("meanCores"), "memoryMiB": whole("memoryMiB"), "peakMemoryMiB": whole("peakMemoryMiB"), "gpuSeconds": rounded("gpuSeconds"),
    })
}

/// Each quiet native lease with whether it still holds batch work and, if so, the batch jobs waiting behind it.
fn quiet_standing(root: &Path, active: &[Value], queue: &[Value], now: f64) -> Vec<Value> {
    let facts = lease_facts(root, &active.iter().collect::<Vec<_>>());
    crate::capacity::native_hold::standing_json(&facts, queue, now)
}

fn holding(root: &Path, lease: &Value, accounting: &super::request::Accounting, quiet: &[Value], now: f64) -> Obj {
    let run = is_run(lease);
    let (committed, basis) = if run { accounting.commitment(id(lease)) } else { (0.0, "agent-memory-only") };
    let get = |key: &str| lease.get(key).cloned().unwrap_or(Value::Null);
    let declared = |key: &str| lease.pointer(&format!("/declared/{key}")).cloned().unwrap_or_else(|| get(key));
    Obj::new()
        .put("owner", get("owner"))
        .put("class", get("class"))
        .put("kind", get("kind"))
        .put("cpus", get("cpus"))
        .put("memoryMiB", get("memoryMiB"))
        .put("declaredCpus", declared("cpus"))
        .put("declaredMemoryMiB", declared("memoryMiB"))
        .put("evidenceRuns", lease.pointer("/evidence/runs").cloned().unwrap_or(json!(0)))
        .put("estimate", get("estimate"))
        .put("committedCpus", if run { num(round2(committed)) } else { json!(0) })
        .put("cpuBasis", basis)
        .put("measured", if run { measured_view(read_usage(root, id(lease))) } else { Value::Null })
        .put_some("pair", lease.get("pair").cloned())
        .put_some("quiet", quiet.iter().find(|entry| text(entry, "id") == Some(id(lease))).cloned())
        .put("heldSeconds", seconds(now - field(lease, "createdAt").unwrap_or(f64::NAN)))
        .put("remainingSeconds", field(lease, "expiresAt").map_or(Value::Null, |expires| seconds(expires - now)))
}

fn queued(queue: Vec<Value>, draining: &[Value], now: f64) -> Vec<Obj> {
    priority_order(queue)
        .iter()
        .enumerate()
        .map(|(index, ticket)| {
            let get = |key: &str| ticket.get(key).cloned().unwrap_or(Value::Null);
            Obj::new()
                .put("position", index + 1)
                .put("owner", get("owner"))
                .put("class", get("class"))
                .put("reason", get("reason"))
                .put("waitingSeconds", seconds(now - field(ticket, "queuedAt").unwrap_or(f64::NAN)))
                .put_some("newClients", ticket.get("newClients").cloned())
                .put_some("newDesktops", ticket.get("newDesktops").cloned())
                .put_some("waitingFor", (class(ticket) == "exclusive").then(|| Value::Array(draining.to_vec())))
        })
        .collect()
}

pub fn status(root: &Path) -> Obj {
    let gpu = read_gpu(1);
    with_lock(root, || {
        let now = now_ms();
        let gpu_busy = record_gpu_busy(root, gpu.busy_percent, now);
        let (mut active, reclaimed) = read_leases(root, now);
        let (mode, profile) = active_profile(root);
        let signals = read_signals();
        let accounting = cpu_accounting(root, &active, now);
        let quiet = quiet_standing(root, &active, &read_queue(root), now);
        let draining: Vec<Value> = active
            .iter()
            .filter(|lease| is_run(lease) && batch_class(class(lease)))
            .map(|lease| json!({ "owner": lease.get("owner"), "class": lease.get("class"), "remainingSeconds": seconds(field(lease, "expiresAt").unwrap_or(f64::NAN) - now) }))
            .collect();
        active.sort_by(|left, right| field(left, "createdAt").unwrap_or(0.0).total_cmp(&field(right, "createdAt").unwrap_or(0.0)));
        let holding: Vec<Obj> = active.iter().map(|lease| holding(root, lease, &accounting, &quiet, now)).collect();
        Obj::new()
            .put("profile", profile)
            .put("mode", mode)
            .merge(gpu_view(gpu))
            .put("warcraftClients", crate::capacity::warcraft::view())
            .merge(gpu_busy)
            .put("aggregateCpuLimit", num(aggregate_cpus(profile, signals.cores)))
            .put("protectedCpuSomeAvg10", num(signals.protected_cpu_some / 100.0))
            .put("cpuSomeAvg10", num(signals.cpu_some / 100.0))
            .put("committedBatchCpus", num(accounting.committed_batch_cpus))
            .put("committedNativeCpus", num(accounting.committed_native_cpus))
            .put("holding", holding.iter().map(Obj::value).collect::<Vec<_>>())
            .put("queued", queued(read_queue(root), &draining, now).iter().map(Obj::value).collect::<Vec<_>>())
            .put("unleasedHeavy", accounting.unleased.clone())
            .put("reclaimed", reclaimed)
    })
}
