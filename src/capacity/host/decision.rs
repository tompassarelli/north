use std::path::Path;

use serde_json::{Value, json};

use super::fail;
use super::gpu::{GpuProcessSample, take_gpu_process_sample, top_unleased_gpu_process};
use super::json::{Obj, field, num, round2, text};
use super::machine::{Gpu, Signals, enclosing_lease, env, lease_slice, now_ms, read_gpu, read_signals, scope_cgroup, wrapper_identity};
use super::policy::{GPU_BUSY_LIMIT_PERCENT, aggregate_cpus, batch_class};
use super::request::{Create, Request, Totals, class, cpu_accounting, grant, granted_view, id, is_run, lease_facts, totals};
use super::state::{RUN_LEASE_GRACE_MILLISECONDS, active_profile, dequeue, lease_path, mark_native_waiting, native_waiting, read_leases, read_queue, record_gpu_busy, with_lock, write_lease};
use super::systemd::scope_unit;
use crate::capacity::Host;

const NATIVE_GPU_SAMPLES: u32 = 10;

pub(crate) fn signals_view(signals: &Signals) -> Obj {
    Obj::new()
        .put("cores", num(signals.cores))
        .put("memoryTotalMiB", num(signals.memory_total_mib))
        .put("memoryAvailableMiB", num(signals.memory_available_mib))
        .put("cpuSomeAvg10BasisPoints", num(signals.cpu_some))
        .put("protectedCpuSomeAvg10BasisPoints", num(signals.protected_cpu_some))
        .put("memoryFullAvg10BasisPoints", num(signals.memory_full))
}

pub(crate) fn gpu_view(gpu: Gpu) -> Obj {
    Obj::new().put("gpuBusyPercent", num(gpu.busy_percent)).put("gpuClients", num(gpu.clients))
}

/// Runs admission on the facts and returns its reason and the rest of its advice.
pub(crate) fn admit(facts: Obj, host: &Host) -> (String, Value) {
    let facts = serde_json::from_value(facts.value()).unwrap_or_else(|error| fail(&format!("north capacity admission failed: {error}"), 75));
    let mut advice = serde_json::to_value(crate::capacity::admit_enriched(facts, host)).unwrap_or(Value::Null);
    let reason = advice.as_object_mut().and_then(|map| map.remove("reason")).and_then(|reason| reason.as_str().map(str::to_owned)).unwrap_or_default();
    (reason, advice)
}

struct Queued {
    ahead: Vec<Value>,
    exclusive_waiting: usize,
}

fn queued_ahead(queue: &[Value], request: &Request, ticket: Option<&str>, before_drain: &dyn Fn(Option<f64>) -> bool) -> Queued {
    let earlier = |entry: &Value| ticket.is_none_or(|ticket| text(entry, "name").is_some_and(|name| name < ticket));
    let exclusive_waiting = queue.iter().filter(|entry| text(entry, "class") == Some("exclusive") && (request.name != "exclusive" || earlier(entry))).count();
    // Gpu tickets wait for render slots and native tickets for client slots, each in their own line, so they hold no CPU work.
    let ahead = queue
        .iter()
        .filter(|entry| {
            let entry_class = text(entry, "class").unwrap_or_default();
            earlier(entry)
                && (entry_class == "gpu") == (request.name == "gpu")
                && (entry_class == "native") == (request.name == "native")
                && entry_class != "exclusive"
                && (matches!(request.name.as_str(), "heavy" | "exclusive") || text(entry, "reason") != Some("DEFER_RUN_QUEUE"))
                && (exclusive_waiting == 0 || entry_class != "heavy" || before_drain(field(entry, "timeoutSeconds")))
        })
        .cloned()
        .collect();
    Queued { ahead, exclusive_waiting }
}

struct Observed {
    now: f64,
    gpu: Gpu,
    gpu_busy: Obj,
    active: Vec<Value>,
    reclaimed: u32,
    queue: Vec<Value>,
}

fn runs(observed: &Observed) -> Vec<&Value> {
    observed.active.iter().filter(|lease| is_run(lease)).collect()
}

fn facts(root: &Path, request: &Request, create: Option<&Create>, ticket: Option<&str>, observed: &Observed, context: &Context) -> Obj {
    let runs = runs(observed);
    let count = |filter: &dyn Fn(&Value) -> bool| runs.iter().filter(|lease| filter(lease)).count();
    let unleased_cores = context.unleased_cores.unwrap_or(0.0);
    signals_view(&context.signals)
        .merge(gpu_view(observed.gpu))
        .put("profile", context.profile)
        .put("class", request.admitted_class())
        .put("gamingClass", request.name.as_str())
        .put("queuedAheadClasses", context.queued.ahead.iter().map(|entry| entry.get("requestedClass").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>())
        .put("queuedAheadNewClients", context.queued.ahead.iter().map(|entry| entry.get("newClients").filter(|value| !value.is_null()).cloned().unwrap_or(json!(0))).collect::<Vec<_>>())
        .put("leasedMemoryMiB", num(context.leased.memory_mib))
        .put("leasedBatchCpus", num(round2(context.committed_batch_cpus - unleased_cores)))
        .put("leasedNativeCpus", num(context.committed_native_cpus))
        .put("unleasedHeavyCores", num(unleased_cores))
        .put("requestedCpus", num(context.charged.0))
        .put("requestedMemoryMiB", num(context.charged.1))
        .put("peerBatchRuns", count(&|lease| batch_class(class(lease))))
        .put("peerExclusiveRuns", count(&|lease| class(lease) == "exclusive"))
        .put("unboundedRuns", count(&|lease| text(lease, "aggregateSlice") != Some(lease_slice(class(lease)).as_str())))
        .put("queuedAhead", context.queued.ahead.len())
        .put("exclusiveWaiting", context.queued.exclusive_waiting)
        .put("beforeDrain", context.before_drain_create)
        .put("newClients", request.new_clients)
        .put("newDesktops", request.new_desktops)
        .put("gpuRuns", count(&|lease| class(lease) == "gpu"))
        .put("nativeWaiting", native_waiting(root, observed.now))
        .put("leases", with_started_clients(root, lease_facts(root, &runs)))
        .put_some("requesterLease", requester_lease(&runs))
        .put("queue", observed.queue.clone())
        .put("now", num(observed.now))
        .put("alone", request.alone)
        .put("owner", create.map(|create| create.owner.clone()).or_else(|| request.owner.clone()))
        .put("pair", request.pair)
        .put("besideNative", request.beside_native.is_some())
        .put("quiet", request.quiet)
        .put("exclusiveLeases", runs.iter().filter(|lease| class(lease) == "exclusive").map(|lease| id(lease)).collect::<Vec<_>>())
        .put_some("ticket", ticket.map(|ticket| root.join("queue").join(ticket).to_string_lossy().into_owned()))
}

/// Marks each lease fact with the Warcraft clients running inside its scope, and records on each native lease the most it has seen running there, so its hold doesn't return once they exit.
fn with_started_clients(root: &Path, mut leases: Vec<Value>) -> Vec<Value> {
    let cgroups = crate::capacity::warcraft::cgroups();
    for lease in &mut leases {
        let scope = scope_unit(id(lease));
        let running = cgroups.iter().filter(|cgroup| cgroup.split('/').any(|part| part == scope)).count() as u64;
        let seen = field(lease, "clientsSeen").unwrap_or(0.0) as u64;
        if let Value::Object(map) = lease {
            map.insert("runningClients".to_owned(), json!(running));
        }
        if running > seen && class(lease) == "native" && field(lease, "newClients").is_some() {
            add_to_lease(root, id(lease), "clientsSeen", running - seen);
            if let Value::Object(map) = lease {
                map.insert("clientsSeen".to_owned(), json!(running));
            }
        }
    }
    leases
}

/// Adds `amount` to a number field of the stored lease `id`; the caller holds the admission lock.
fn add_to_lease(root: &Path, id: &str, key: &str, amount: u64) {
    let path = lease_path(root, id);
    if let Some(stored) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
        let total = field(&stored, key).unwrap_or(0.0) as u64 + amount;
        write_lease(&path, &Obj::new().spread(&stored).put(key, total), false);
    }
}

/// The live native run lease whose scope this request comes from, such as a pair session launching its own clients.
fn requester_lease(runs: &[&Value]) -> Option<String> {
    let scope = enclosing_lease()?;
    runs.iter().find(|lease| class(lease) == "native" && id(lease) == scope).map(|lease| id(lease).to_owned())
}

struct Context {
    signals: Signals,
    profile: &'static str,
    mode: String,
    queued: Queued,
    leased: Totals,
    committed_batch_cpus: f64,
    committed_native_cpus: f64,
    unleased_cores: Option<f64>,
    charged: (f64, f64),
    before_drain_create: bool,
}

fn result(request: &Request, create: Option<&Create>, code: &str, observed: &Observed, context: &Context, granted: &super::request::Granted) -> Obj {
    let decision = match code {
        "RUN" if create.is_some() => "RESERVED",
        "RUN" => "RUN",
        "PAIR_HELD" => "REFUSED",
        _ => "DEFER",
    };
    let declared = granted.declared.map(|declared| declared.json());
    let scope = declared.clone().unwrap_or_else(|| granted.charged.json());
    let signals = &context.signals;
    Obj::new()
        .put("decision", decision)
        .put("reason", code)
        .put("profile", context.profile)
        .put("mode", context.mode.as_str())
        .put("class", request.name.as_str())
        .merge(granted_view(granted))
        .put("scope", scope)
        .put("leasedCpuCeilings", num(context.leased.cpus))
        .put("leasedBatchCpus", num(context.leased.batch_cpus))
        .put_some("sizing", request.sizing.clone())
        .put_some("declaredCpus", granted.declared.map(|declared| num(declared.cpus)))
        .put_some("declaredMemoryMiB", granted.declared.map(|declared| num(declared.memory_mib)))
        .put_some("evidence", granted.declared.map(|_| request.evidence.clone().unwrap_or(Value::Null)))
        .put("aggregateCpuLimit", num(aggregate_cpus(context.profile, signals.cores)))
        .put("queuedAhead", context.queued.ahead.len())
        .put("exclusiveWaiting", context.queued.exclusive_waiting)
        .put("leasedNativeCpus", num(context.leased.native_cpus))
        .put("leasedMemoryMiB", num(context.leased.memory_mib))
        .put("protectedCpuSomeAvg10", num(signals.protected_cpu_some / 100.0))
        .put("cpuSomeAvg10", num(signals.cpu_some / 100.0))
        .put("memoryFullAvg10", num(signals.memory_full / 100.0))
        .put("memoryAvailableMiB", num(signals.memory_available_mib))
        .merge(gpu_view(observed.gpu))
}

fn gpu_samples(request: &Request) -> (Option<GpuProcessSample>, Gpu) {
    let sampled = request.name == "native" && env("AGENT_CAPACITY_GPU_BUSY").is_none();
    let before = sampled.then(take_gpu_process_sample);
    let gpu = read_gpu(if sampled { NATIVE_GPU_SAMPLES } else { 1 });
    (before, gpu)
}

fn observe(root: &Path, gpu: Gpu, first_sample_at: Option<f64>) -> Observed {
    let now = now_ms();
    let gpu_busy = record_gpu_busy(root, gpu.busy_percent, first_sample_at.unwrap_or(now));
    let (active, reclaimed) = read_leases(root, now);
    let queue = read_queue(root);
    Observed { now, gpu, gpu_busy, active, reclaimed, queue }
}

fn context(root: &Path, request: &Request, create: Option<&Create>, ticket: Option<&str>, observed: &Observed) -> (Context, super::request::Granted, super::request::Accounting) {
    let runs = runs(observed);
    let now = observed.now;
    let drain_ends_at = runs.iter().filter(|lease| batch_class(class(lease))).filter_map(|lease| field(lease, "expiresAt")).fold(now, f64::max);
    let before_drain = |seconds: Option<f64>| seconds.is_some_and(|seconds| seconds.fract() == 0.0 && now + seconds * 1000.0 + RUN_LEASE_GRACE_MILLISECONDS <= drain_ends_at);
    let queued = queued_ahead(&observed.queue, request, ticket, &before_drain);
    let accounting = cpu_accounting(root, &observed.active, now);
    let signals = read_signals();
    let (mode, profile) = active_profile(root);
    let granted = grant(profile, signals.cores, signals.memory_total_mib, request);
    let context = Context {
        signals,
        profile,
        mode,
        queued,
        leased: totals(&observed.active),
        committed_batch_cpus: accounting.committed_batch_cpus,
        committed_native_cpus: accounting.committed_native_cpus,
        unleased_cores: accounting.unleased_cores(),
        charged: (granted.charged.cpus, granted.charged.memory_mib),
        before_drain_create: before_drain(create.and_then(|create| create.timeout_seconds).map(|seconds| seconds as f64)),
    };
    (context, granted, accounting)
}

/// Admits, defers or refuses the request under the shared lock, writing its lease when it is admitted with a lease to create.
pub(crate) fn decision(root: &Path, request: &Request, create: Option<&Create>, ticket: Option<&str>, host: &Host) -> Obj {
    let (before, gpu) = gpu_samples(request);
    let after = before.as_ref().filter(|_| gpu.busy_percent >= GPU_BUSY_LIMIT_PERCENT).map(|_| take_gpu_process_sample());
    with_lock(root, || {
        let observed = observe(root, gpu, before.as_ref().map(|sample| sample.at));
        let (context, granted, accounting) = context(root, request, create, ticket, &observed);
        let (code, advice) = admit(facts(root, request, create, ticket, &observed, &context), host);
        if request.name == "native" {
            mark_native_waiting(root, code != "RUN" && code != "PAIR_HELD");
        }
        let mut outcome = result(request, create, &code, &observed, &context, &granted);
        if code == "DEFER_GPU_BUSY" {
            let leased: Vec<String> = runs(&observed).iter().map(|lease| scope_cgroup(class(lease), &scope_unit(id(lease))).to_string_lossy().chars().skip(super::machine::cgroup_root().chars().count()).collect()).collect();
            let process = before.as_ref().zip(after.as_ref()).map_or(Value::Null, |(before, after)| top_unleased_gpu_process(before, after, &leased));
            outcome = outcome.merge(observed.gpu_busy.clone()).put("gpuProcess", process);
        }
        outcome = outcome
            .spread(&advice)
            .put("gpuLeases", runs(&observed).iter().filter(|lease| class(lease) == "gpu").count())
            .put("unleasedHeavyCores", super::json::opt(accounting.unleased_cores()))
            .put("committedBatchCpus", num(context.committed_batch_cpus))
            .put("committedNativeCpus", num(context.committed_native_cpus))
            .put("reclaimed", observed.reclaimed);
        match create.filter(|_| code == "RUN") {
            Some(create) => create_lease(root, request, create, ticket, &granted, requester_lease(&runs(&observed)), outcome),
            None => outcome,
        }
    })
}

fn create_lease(root: &Path, request: &Request, create: &Create, ticket: Option<&str>, granted: &super::request::Granted, parent: Option<String>, outcome: Obj) -> Obj {
    if let Some(ticket) = ticket {
        dequeue(root, ticket);
    }
    let id = super::machine::uuid();
    let now = now_ms();
    let run = create.kind == "run";
    let expires_at = create.timeout_seconds.map(|seconds| now + seconds as f64 * 1000.0 + if run { RUN_LEASE_GRACE_MILLISECONDS } else { 0.0 });
    let mut lease = Obj::new()
        .put("schema", "agent-capacity-lease/v1")
        .put("id", id.as_str())
        .put("kind", create.kind)
        .put_some("quiet", request.quiet.then_some(true))
        .put_some("quietSeconds", request.quiet_seconds)
        .put_some("pair", request.pair)
        .put("class", request.name.as_str())
        .put_some("newClients", (request.name == "native" && request.new_clients > 0).then_some(request.new_clients))
        .put_some("parentLease", parent.as_deref())
        .put("aggregateSlice", run.then(|| lease_slice(&request.name)))
        .put("owner", create.owner.as_str())
        .put("cpus", num(granted.charged.cpus))
        .put("memoryMiB", num(granted.charged.memory_mib))
        .put_some("estimate", request.estimate.clone())
        .put_some("declared", granted.declared.map(|declared| declared.json()))
        .put_some("evidence", granted.declared.map(|_| request.evidence.clone().unwrap_or(Value::Null)))
        .put("createdAt", num(now))
        .put("expiresAt", super::json::opt(expires_at));
    if run {
        let (pid, start) = wrapper_identity();
        lease = lease.put("wrapperPid", pid).put("wrapperStart", start);
    }
    write_lease(&lease_path(root, &id), &lease, true);
    if let Some(parent) = parent.as_deref().filter(|_| request.name == "native" && request.new_clients > 0) {
        add_to_lease(root, parent, "clientsStarted", u64::from(request.new_clients));
    }
    outcome.put("lease", id).put("expiresAt", super::json::opt(expires_at))
}
