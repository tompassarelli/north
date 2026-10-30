use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::decision::decision;
use super::estimate::task_signature;
use super::json::{Obj, field, num, round2, text};
use super::machine::{env, lease_slice, now_ms, read_signals, read_text, scope_cgroup};
use super::measure::{Usage, measure_scope};
use super::policy::batch_class;
use super::request::{Create, Request, class, id, is_run};
use super::shape::{command_shape, recorded_command};
use super::state::{active_profile, dequeue, enqueue, exact_lease, read_leases, with_lock};
use super::systemd::{apply_profile, client_environment, scope_live, scope_unit, stop_scope};
use super::{change_lease, fail, print_to};
use crate::capacity::Host;
use crate::capacity::build_offload::{self, Candidate, Job};

const QUEUE_POLL_MILLISECONDS: u64 = 1000;
const OFFLOAD_RETRY_MILLISECONDS: f64 = 15_000.0;
const OFFLOAD_DEFAULT_TIMEOUT_SECONDS: u64 = 3600;
const FROZEN_CLASSES: [&str; 3] = ["heavy", "exclusive", "gpu"];
const USER_HZ: f64 = 100.0;

struct Terminations([Option<Signal>; 3]);

impl Terminations {
    fn listen() -> Self {
        Self([SignalKind::interrupt(), SignalKind::terminate(), SignalKind::hangup()].map(|kind| signal(kind).ok()))
    }

    async fn next(&mut self) -> i32 {
        async fn one(signal: &mut Option<Signal>) {
            match signal {
                Some(signal) => drop(signal.recv().await),
                None => std::future::pending().await,
            }
        }
        let [interrupt, terminate, hangup] = &mut self.0;
        tokio::select! {
            () = one(interrupt) => 2,
            () = one(terminate) => 15,
            () = one(hangup) => 1,
        }
    }
}

fn leases_changed(root: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(root.join("leases")).and_then(|metadata| metadata.modified()).ok()
}

/// Waits until a lease file appears or goes, or the jittered poll interval passes.
async fn lease_change(root: &Path) {
    let jitter = u64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos())) % QUEUE_POLL_MILLISECONDS;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(QUEUE_POLL_MILLISECONDS + jitter);
    let before = leases_changed(root);
    while tokio::time::Instant::now() < deadline && leases_changed(root) == before {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn reason(result: &Obj) -> Option<String> {
    result.get("reason").and_then(Value::as_str).map(str::to_owned)
}

enum Admitted {
    Here(Obj),
    Offloaded(u8),
}

struct Waiting<'a> {
    root: &'a Path,
    ticket: Option<&'a str>,
    request: &'a Request,
    create: &'a Create,
    command: &'a [String],
}

/// A heavy build this host defers runs on another host with room instead of queueing here; None leaves it on the local path.
async fn try_offload(waiting: Waiting<'_>, deferred: &Obj, host: &Host, terminations: &mut Terminations) -> Option<u8> {
    let Waiting { root, ticket, request, create, command } = waiting;
    let host = *host;
    let (root, ticket) = (root.to_path_buf(), ticket.map(str::to_owned));
    let (waiting_root, waiting_ticket) = (root.clone(), ticket.clone());
    let landing = request.landing || env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty());
    let (class, owner, command) = (request.name.clone(), create.owner.clone(), command.to_vec());
    let timeout_seconds = create.timeout_seconds.unwrap_or(OFFLOAD_DEFAULT_TIMEOUT_SECONDS);
    let (deferred, need) = (reason(deferred).is_some_and(|reason| reason.starts_with("DEFER_")), env("AGENT_CAPACITY_NEED").filter(|need| !need.is_empty()));
    let offloaded_already = env(build_offload::REMOTE_MARK).is_some();
    let work = tokio::task::spawn_blocking(move || {
        let candidate = Candidate { class: &class, landing, need: need.as_deref(), deferred, offloaded_already };
        let announce = |target: &str| {
            print_to(&Obj::new().put("decision", "OFFLOADED").put("reason", build_offload::REASON).put("host", target).put("class", class.as_str()).put("owner", owner.as_str()), true);
            if let Some(ticket) = &ticket {
                dequeue(&root, ticket);
            }
        };
        (host.build_offload)(&candidate, &Job { owner: &owner, timeout_seconds, command: &command }, &announce)
    });
    tokio::select! {
        done = work => done.ok().flatten(),
        number = terminations.next() => {
            if let Some(ticket) = &waiting_ticket {
                dequeue(&waiting_root, ticket);
            }
            std::process::exit(128 + number)
        }
    }
}

/// A heavy request its host defers on the first look runs elsewhere when a host has room; otherwise None sends it to the queue.
async fn offer_first(root: &Path, request: &Request, create: &Create, command: &[String], host: &Host, terminations: &mut Terminations) -> Option<Admitted> {
    let first = decision(root, request, Some(create), None, host);
    if first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return Some(Admitted::Here(first));
    }
    try_offload(Waiting { root, ticket: None, request, create, command }, &first, host, terminations).await.map(Admitted::Offloaded)
}

fn wait_ticket(root: &Path, request: &Request, create: &Create) -> String {
    enqueue(
        root,
        Obj::new()
            .put("class", request.admitted_class())
            .put("requestedClass", request.name.as_str())
            .put("requestedCpus", num(request.resources().cpus))
            .put("owner", create.owner.as_str())
            .put("timeoutSeconds", create.timeout_seconds)
            .put("newClients", request.new_clients)
            .put("newDesktops", request.new_desktops)
            .put_some("alone", request.alone.then_some(true)),
    )
}

/// Batch work, and a native client or desktop start given --wait-seconds, is held in arrival order until admitted, its --wait-seconds pass (ending in its last DEFER), or a quiet native session outlasts its timeout (REFUSED).
async fn admit(root: &Path, request: &Request, create: &Create, command: &[String], host: &Host, terminations: &mut Terminations) -> Admitted {
    if !batch_class(&request.name) && request.wait_seconds.is_none() && !request.alone {
        return Admitted::Here(decision(root, request, Some(create), None, host));
    }
    let offer = request.name == "heavy" && !request.landing;
    if offer && let Some(admitted) = offer_first(root, request, create, command, host, terminations).await {
        return admitted;
    }
    let mut offered_at = now_ms();
    let queued_at = now_ms();
    let ticket = wait_ticket(root, request, create);
    let mut reported = None;
    loop {
        let result = decision(root, request, Some(create), Some(&ticket), host);
        let queued_seconds = round2((now_ms() - queued_at) / 1000.0);
        let result = result.put("queuedSeconds", num(queued_seconds));
        let decided = result.get("decision").and_then(Value::as_str).unwrap_or_default();
        if decided == "RESERVED" || decided == "REFUSED" || request.wait_seconds.is_some_and(|wait| queued_seconds >= wait as f64) {
            dequeue(root, &ticket);
            return Admitted::Here(result);
        }
        if reason(&result).as_deref() == Some("DEFER_NATIVE_QUIET") && create.timeout_seconds.is_some_and(|timeout| queued_seconds >= timeout as f64) {
            dequeue(root, &ticket);
            return Admitted::Here(result.put("decision", "REFUSED"));
        }
        if offer && now_ms() - offered_at >= OFFLOAD_RETRY_MILLISECONDS {
            offered_at = now_ms();
            if let Some(code) = try_offload(Waiting { root, ticket: Some(&ticket), request, create, command }, &result, host, terminations).await {
                dequeue(root, &ticket);
                return Admitted::Offloaded(code);
            }
        }
        if reason(&result) != reported {
            print_to(&result.clone().put("decision", "QUEUED"), true);
            reported = reason(&result);
        }
        tokio::select! {
            () = lease_change(root) => {}
            number = terminations.next() => {
                dequeue(root, &ticket);
                std::process::exit(128 + number);
            }
        }
    }
}

/// The batch lease this process already runs inside, by its environment or its cgroup scope.
fn nested_parent(root: &Path) -> Option<String> {
    let ids = [env("AGENT_CAPACITY_LEASE"), super::machine::enclosing_lease()];
    with_lock(root, || {
        let (active, _) = read_leases(root, now_ms());
        active
            .iter()
            .find(|lease| is_run(lease) && batch_class(class(lease)) && ids.iter().flatten().any(|candidate| candidate == id(lease)))
            .map(|lease| id(lease).to_owned())
    })
}

fn timeout_text(timeout_seconds: Option<u64>) -> String {
    timeout_seconds.map_or_else(|| "null".to_owned(), |seconds| seconds.to_string())
}

pub(crate) fn north_command() -> String {
    env("AGENT_CAPACITY_NORTH")
        .or_else(|| std::env::current_exe().ok().map(|path| path.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("{}/.local/state/north/current/north", env("HOME").unwrap_or_default()))
}

fn scope_command(root: &Path, request: &Request, admitted: &Obj, timeout_seconds: Option<u64>, command: &[String]) -> tokio::process::Command {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let north = north_command();
    let mut arguments: Vec<String> = ["--user", "--scope", "--quiet", "--collect", "--expand-environment=no"].map(str::to_owned).to_vec();
    if let (Some(runtime), Some(bus)) = (env("XDG_RUNTIME_DIR"), env("DBUS_SESSION_BUS_ADDRESS")) {
        arguments.extend([format!("--setenv=XDG_RUNTIME_DIR={runtime}"), format!("--setenv=DBUS_SESSION_BUS_ADDRESS={bus}")]);
    }
    arguments.extend([format!("--unit={unit}"), format!("--slice={}", lease_slice(&request.name))]);
    let scope = admitted.get("scope").cloned().unwrap_or(Value::Null);
    // A quota-throttled game client stalls for the rest of each period.
    if request.name != "native" {
        arguments.push(format!("--property=CPUQuota={}%", super::json::show(field(&scope, "cpus").unwrap_or(f64::NAN) * 100.0)));
    }
    arguments.extend([format!("--property=MemoryHigh={}M", super::json::show(field(&scope, "memoryMiB").unwrap_or(f64::NAN))), "--property=KillMode=control-group".to_owned()]);
    if FROZEN_CLASSES.contains(&request.name.as_str()) {
        arguments.extend(["--".to_owned(), north.clone(), "capacity".to_owned(), "deadline".to_owned(), exact_lease(root, &lease).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()]);
    } else {
        arguments.extend([format!("--property=RuntimeMaxSec={}", timeout_seconds.map_or_else(|| "infinity".to_owned(), |seconds| format!("{seconds}s"))), "--".to_owned()]);
    }
    arguments.extend(command.iter().cloned());
    let mut child = if env("NORTH_FLEET_SCOPE").is_some_and(|scope| !scope.is_empty()) {
        let mut child = std::process::Command::new(&north);
        child.arg("session-unit");
        child
    } else {
        std::process::Command::new("systemd-run")
    };
    child.args(arguments).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).env("AGENT_CAPACITY_LEASE", &lease);
    if let Some(pair) = request.pair {
        child.env("AGENT_CAPACITY_PAIR", pair.to_string());
    }
    if request.name != "native" {
        let jobs = field(&scope, "cpus").filter(|cpus| cpus.is_finite()).map_or(1.0, f64::ceil).max(1.0).to_string();
        child.env("CARGO_BUILD_JOBS", &jobs).env("RUST_TEST_THREADS", &jobs);
    }
    client_environment(&mut child);
    tokio::process::Command::from(child)
}

fn children_cpu_seconds() -> f64 {
    let stat = read_text("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.get(stat.rfind(')').map_or(0, |end| end + 2)..).unwrap_or_default().split(' ').collect();
    let ticks = |index: usize| fields.get(index).and_then(|field| field.parse::<f64>().ok()).unwrap_or(0.0);
    (ticks(13) + ticks(14)) / USER_HZ
}

fn usage_entry(owner: &str, request: &Request, command: &[String], admitted: &Obj, exit_code: i32, usage: &Usage, child_cpu: f64) -> Obj {
    let cpu_seconds = usage.cpu_seconds.max(child_cpu);
    let mean = cpu_seconds / usage.wall_seconds.max(0.001);
    let scope = admitted.get("scope").cloned().unwrap_or(Value::Null);
    let get = |key: &str| admitted.get(key).cloned().unwrap_or(Value::Null);
    Obj::new()
        .put("at", crate::foundation::clock::Timestamp::now().iso_millis())
        .put("owner", owner)
        .put("shape", command_shape(command))
        .put("command", recorded_command(command))
        .put("class", request.name.as_str())
        .put("reservedCpus", scope.get("cpus").cloned().unwrap_or(Value::Null))
        .put("reservedMemoryMiB", scope.get("memoryMiB").cloned().unwrap_or(Value::Null))
        .put("chargedCpus", get("requestedCpus"))
        .put("chargedMemoryMiB", get("requestedMemoryMiB"))
        .put("exitCode", exit_code)
        .put("wallSeconds", num(round2(usage.wall_seconds)))
        .put("cpuSeconds", num(round2(cpu_seconds)))
        .put("meanCores", num(round2(mean)))
        .put("p90Cores", num(round2(usage.p90_cores.unwrap_or(mean))))
        .put("peakCores", num(round2(usage.peak_cores.max(mean))))
        .put("peakMemoryMiB", num((usage.peak_memory_mib + 0.5).floor()))
        .put("gpuSeconds", num(round2(usage.gpu_seconds)))
        .put("queuedSeconds", admitted.get("queuedSeconds").cloned().unwrap_or(serde_json::json!(0)))
        .put_some("besideNative", request.beside_native.clone())
}

fn record(entry: &Obj, usage: &Usage, command: &[String], request: &Request, lease: &str) {
    let cwd = std::env::current_dir().map(|cwd| cwd.to_string_lossy().into_owned()).ok();
    let row = entry
        .clone()
        .put("ioReadBytes", super::json::opt(usage.io_read_bytes))
        .put("ioWriteBytes", super::json::opt(usage.io_write_bytes))
        .put("ioPressureSomeUsec", super::json::opt(usage.io_pressure_some_usec))
        .put("diskBusyShare", super::json::opt(usage.disk_busy_share));
    let stored = super::database::capacity_database().and_then(|db| {
        super::database::insert_capacity_run(&db, &format!("lease:{lease}"), &row.value(), &task_signature(command, cwd.as_deref(), &request.name, true), cwd.as_deref())
    });
    if let Err(error) = stored {
        eprintln!("machine-capacity: capacity database failed: {error}");
    }
    if let Err(error) = super::database::record_usage(&entry.text()) {
        eprintln!("machine-capacity: usage log failed: {error}");
    }
}

async fn supervise(mut child: tokio::process::Child, unit: &str, terminations: &mut Terminations) -> i32 {
    loop {
        tokio::select! {
            status = child.wait() => {
                return status.map_or(1, |status| {
                    use std::os::unix::process::ExitStatusExt;
                    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1)
                });
            }
            _ = terminations.next() => stop_scope(unit),
        }
    }
}

async fn run_admitted(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], admitted: &Obj, terminations: &mut Terminations) -> i32 {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let child_cpu_before = children_cpu_seconds();
    let exit_code = match scope_command(root, request, admitted, timeout_seconds, command).spawn() {
        Ok(child) => {
            let measurement = measure_scope(root, &lease, scope_cgroup(&request.name, &unit));
            let exit_code = supervise(child, &unit, terminations).await;
            let usage = measurement.finish();
            let entry = usage_entry(owner, request, command, admitted, exit_code, &usage, children_cpu_seconds() - child_cpu_before);
            record(&entry, &usage, command, request, &lease);
            exit_code
        }
        Err(error) => {
            eprintln!("machine-capacity: {error}");
            1
        }
    };
    stop_scope(&unit);
    if scope_live(&lease) {
        eprintln!("machine-capacity: lease cleanup failed: scope still live; retained lease {lease}");
    } else {
        print_to(&change_lease(root, &lease, owner, None, true), true);
    }
    exit_code
}

fn refusal(admitted: &Obj) -> Option<String> {
    let view = admitted.value();
    if text(&view, "reason") == Some("PAIR_HELD") {
        let holder = view.get("holder").map_or_else(String::new, |holder| holder.as_str().map_or_else(|| holder.to_string(), str::to_owned));
        return Some(format!("refused: pair {} is held by {holder}; free pair: {}", view["pair"], view["freePair"]));
    }
    (text(&view, "decision") == Some("REFUSED")).then(|| {
        let owners: Vec<String> = view["quietOwners"].as_array().map_or_else(Vec::new, |owners| owners.iter().map(|owner| owner.as_str().unwrap_or_default().to_owned()).collect());
        format!("refused after {}s: native evidence run {} is live; retry after it ends", view["queuedSeconds"], owners.join(", "))
    })
}

pub(crate) fn run_scoped(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], host: &Host) -> u8 {
    if request.name == "agent" {
        fail("run --class must be moderate, heavy, exclusive, gpu, critical, tom, runtime, or native", 2);
    }
    if let Some(parent) = batch_class(&request.name).then(|| nested_parent(root)).flatten() {
        print_to(&Obj::new().put("decision", "NESTED").put("parent", parent.as_str()).put("class", request.name.as_str()).put("owner", owner), true);
        let mut arguments = vec![exact_lease(root, &parent).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()];
        arguments.extend(command.iter().cloned());
        return crate::capacity::deadline::run(&arguments);
    }
    // The parent limit and weights must exist before any admitted command executes.
    if !apply_profile(active_profile(root).1, read_signals().cores) {
        fail("cannot establish aggregate CPU limit and native weight", 75);
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap_or_else(|error| fail(&error.to_string(), 1));
    let create = Create { kind: "run", owner: owner.to_owned(), timeout_seconds };
    runtime.block_on(async {
        let mut terminations = Terminations::listen();
        let admitted = match admit(root, request, &create, command, host, &mut terminations).await {
            Admitted::Here(admitted) => admitted,
            Admitted::Offloaded(code) => return code,
        };
        print_to(&admitted, true);
        if let Some(message) = refusal(&admitted) {
            fail(&message, 75);
        }
        if admitted.get("decision").and_then(Value::as_str) != Some("RESERVED") {
            return 75;
        }
        u8::try_from(run_admitted(root, request, owner, timeout_seconds, command, &admitted, &mut terminations).await & 0xff).unwrap_or(1)
    })
}
