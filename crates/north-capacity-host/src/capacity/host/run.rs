use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::decision::decision;
use super::estimate::task_signature;
use super::json::{Obj, field, num, round2, text};
use super::machine::{elastic_class, env, lease_slice, now_ms, read_signals, read_text, scope_cgroup};
use super::measure::{Usage, measure_scope};
use super::model::{Observation, Subject, censored, command_key, current_cache_state, update};
use crate::capacity::local_machine::host_name;
use super::policy::batch_class;
use super::quiet_holds::{QuietHolds, quiet_owners};
use super::requeue;
use super::request::{Create, Request, class, id, is_run};
use super::shape::{command_shape, recorded_command};
use super::state::{active_profile, dequeue, enqueue, exact_lease, read_leases, with_lock};
use super::systemd::{apply_profile, client_environment, scope_live, scope_unit, stop_scope};
use super::{change_lease, fail, print_to};
use crate::capacity::effects::Host;
use crate::capacity::build_offload::{self, Candidate, Declined, Job};
use crate::capacity::dev_shell;

const QUEUE_POLL_MILLISECONDS: u64 = 1000;
const BEHIND_POLL_MILLISECONDS: u64 = 10_000;
pub(super) const OFFLOAD_RETRY_MILLISECONDS: f64 = 15_000.0;
const OFFLOAD_DEFAULT_TIMEOUT_SECONDS: u64 = 3600;
const NO_BUILD_HOST: &str = "NO_BUILD_HOST";
const FROZEN_CLASSES: [&str; 4] = ["heavy", "moderate", "exclusive", "gpu"];
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

fn modified(directory: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(directory).and_then(|metadata| metadata.modified()).ok()
}

/// The lease files present, by name: each running lease's usage sample is rewritten beside them about twice a second, so the directory's modification time says nothing about a lease appearing or going.
fn lease_names(root: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(root.join("leases"))
        .map(|entries| entries.flatten().map(|entry| entry.file_name()).filter(|name| Path::new(name).extension().is_some_and(|extension| extension == "json")).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// How a waiting ticket watches for the next decision: one held behind an earlier ticket (DEFER_QUEUED) can only move once a lease or a ticket appears, goes or records a new reason, so it decides again on such a change or every 10 s; any other waits for a lease to appear or go or the 1 s poll that reads falling pressure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Watch {
    pub poll: Duration,
    pub queue: bool,
}

pub(super) fn watch(reason: Option<&str>) -> Watch {
    if reason == Some("DEFER_QUEUED") {
        Watch { poll: Duration::from_millis(BEHIND_POLL_MILLISECONDS), queue: true }
    } else {
        Watch { poll: Duration::from_millis(QUEUE_POLL_MILLISECONDS), queue: false }
    }
}

/// What a waiter compares to see a decision-relevant change: the lease file names, plus the queue directory's modification time for a ticket held behind an earlier one.
pub(super) fn watched_state(root: &Path, watch: Watch) -> (Vec<std::ffi::OsString>, Option<std::time::SystemTime>) {
    (lease_names(root), watch.queue.then(|| modified(&root.join("queue"))).flatten())
}

/// Waits until the watched state changes or the jittered poll interval passes.
pub(super) async fn state_change(root: &Path, watch: Watch) {
    let jitter = u64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos())) % QUEUE_POLL_MILLISECONDS;
    let deadline = tokio::time::Instant::now() + watch.poll + Duration::from_millis(jitter);
    let before = watched_state(root, watch);
    while tokio::time::Instant::now() < deadline && watched_state(root, watch) == before {
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
    declined: &'a mut Declines,
}

impl Waiting<'_> {
    fn again(&mut self) -> Waiting<'_> {
        Waiting { root: self.root, ticket: self.ticket, request: self.request, create: self.create, command: self.command, declined: &mut *self.declined }
    }
}

/// What the offload offers so far have declined for: the reason last printed, which an unchanged decline stays silent about, and how long the next offer waits, stretched while every offload host is out of reach so a queued build stops probing hosts this host has no path to.
pub(super) struct Declines {
    printed: Option<String>,
    missing_host: Option<Declined>,
    pub(super) retry_milliseconds: f64,
}

impl Declines {
    pub(super) fn new() -> Self {
        Self { printed: None, missing_host: None, retry_milliseconds: OFFLOAD_RETRY_MILLISECONDS }
    }

    /// The OFFLOAD_DECLINED line a decline earns, None when it repeats the reason already printed.
    pub(super) fn note(&mut self, why: Option<Declined>, owner: &str) -> Option<Obj> {
        let why = why?;
        self.missing_host = why.lacks_host().then(|| why.clone());
        self.retry_milliseconds = why.retry_milliseconds(OFFLOAD_RETRY_MILLISECONDS);
        why.changes(&mut self.printed).then(|| Obj::new().put("decision", "OFFLOAD_DECLINED").put("reason", why.reason()).put("detail", why.detail()).put("owner", owner))
    }
}

fn report_declined(why: Option<Declined>, create: &Create, declines: &mut Declines) {
    if let Some(line) = declines.note(why, create.owner.as_str()) {
        print_to(&line, true);
    }
}

/// A heavy build this host defers runs on another host with room instead of queueing here; None leaves it on the local path, printing why once per change of reason.
async fn try_offload(waiting: Waiting<'_>, deferred: bool, host: &Host, terminations: &mut Terminations) -> Option<u8> {
    let Waiting { root, ticket, request, create, command, declined } = waiting;
    let host = *host;
    let (root, ticket) = (root.to_path_buf(), ticket.map(str::to_owned));
    let (waiting_root, waiting_ticket) = (root.clone(), ticket.clone());
    let landing = request.landing || env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty());
    let (class, owner, command) = (request.name.clone(), create.owner.clone(), build_offload::for_build_host(command));
    let timeout_seconds = create.timeout_seconds.unwrap_or(OFFLOAD_DEFAULT_TIMEOUT_SECONDS);
    let need = env("AGENT_CAPACITY_NEED").filter(|need| !need.is_empty());
    let agent = agent_session();
    let offloaded_already = env(build_offload::REMOTE_MARK).is_some();
    let work = tokio::task::spawn_blocking(move || {
        let candidate = Candidate { class: &class, landing, need: need.as_deref(), deferred, agent, offloaded_already };
        let announce = |target: &str| {
            print_to(&Obj::new().put("decision", "OFFLOADED").put("reason", build_offload::REASON).put("host", target).put("class", class.as_str()).put("owner", owner.as_str()), true);
            if let Some(ticket) = &ticket {
                dequeue(&root, ticket);
            }
        };
        (host.build_offload)(&candidate, &Job { owner: &owner, timeout_seconds, command: &command }, &announce)
    });
    tokio::select! {
        done = work => match done.ok()? {
            Ok(code) => Some(code),
            Err(why) => {
                report_declined(why, create, declined);
                None
            }
        },
        number = terminations.next() => {
            if let Some(ticket) = &waiting_ticket {
                dequeue(&waiting_root, ticket);
            }
            std::process::exit(128 + number)
        }
    }
}

/// Whether this process runs inside a fleet session scope, which gets a session unit of its own for its leases.
pub(super) fn fleet_session() -> bool {
    env("NORTH_FLEET_SCOPE").is_some_and(|scope| !scope.is_empty())
}

/// Whether this build was started by a fleet session that has no local need: its compiles belong on a build host before this one.
pub(crate) fn agent_session() -> bool {
    build_offload::is_agent_build(env)
}

/// The decision that ends a fleet session's compile on a host that does not compile for sessions, once no build host took it.
fn no_build_host(missing: &Declined, create: &Create) -> Obj {
    Obj::new().put("decision", "REFUSED").put("reason", NO_BUILD_HOST).put("detail", format!("{}: {}", missing.reason(), missing.detail())).put("owner", create.owner.as_str())
}

/// A fleet session's heavy request is offered to the build hosts before this host decides on it, so a build this host would admit still runs where there is room; any other heavy request runs elsewhere only when its host defers it on the first look. None sends it to the queue.
async fn offer_first(mut waiting: Waiting<'_>, host: &Host, terminations: &mut Terminations) -> Option<Admitted> {
    let (root, request, create) = (waiting.root, waiting.request, waiting.create);
    if agent_session() {
        if let Some(code) = try_offload(waiting.again(), false, host, terminations).await {
            return Some(Admitted::Offloaded(code));
        }
        if !(host.lease_builds)() && build_offload::is_rust_command(waiting.command) && let Some(missing) = waiting.declined.missing_host.as_ref() {
            return Some(Admitted::Here(no_build_host(missing, create)));
        }
    }
    let first = decision(root, request, Some(create), None, host);
    if first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return Some(Admitted::Here(first));
    }
    if agent_session() {
        return None;
    }
    try_offload(waiting, true, host, terminations).await.map(Admitted::Offloaded)
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
            .put_some("alone", request.alone.then_some(true))
            .put_some("priority", request.priority),
    )
}

/// Batch work, and a native client or desktop start given --wait-seconds, is held in arrival order until admitted, its --wait-seconds pass (ending in its last DEFER), or one quiet native session alone holds it past its timeout (REFUSED).
async fn admit(root: &Path, request: &Request, create: &Create, command: &[String], host: &Host, terminations: &mut Terminations) -> Admitted {
    if !batch_class(&request.name) && request.wait_seconds.is_none() && !request.alone {
        return Admitted::Here(decision(root, request, Some(create), None, host));
    }
    let offer = (request.name == "heavy" || request.name == "moderate" && build_offload::is_rust_command(command)) && !request.landing;
    let mut declined = Declines::new();
    let resumed = requeue::resumed(root);
    if resumed.is_none() && offer && let Some(admitted) = offer_first(Waiting { root, ticket: None, request, create, command, declined: &mut declined }, host, terminations).await {
        return admitted;
    }
    let mut offered_at = now_ms();
    let follow = requeue::Follow::here();
    let (ticket, queued_at) = resumed.map_or_else(|| (wait_ticket(root, request, create), now_ms()), |resumed| (resumed.ticket, resumed.queued_at));
    let mut reported = None;
    let mut quiet = QuietHolds::default();
    loop {
        let result = decision(root, request, Some(create), Some(&ticket), host);
        let queued_seconds = round2((now_ms() - queued_at) / 1000.0);
        let result = result.put("queuedSeconds", num(queued_seconds));
        let decided = result.get("decision").and_then(Value::as_str).unwrap_or_default();
        if decided == "RESERVED" || decided == "REFUSED" || request.wait_seconds.is_some_and(|wait| queued_seconds >= wait as f64) {
            dequeue(root, &ticket);
            return Admitted::Here(result);
        }
        if quiet.outlasts(&result.value(), now_ms(), create.timeout_seconds) {
            dequeue(root, &ticket);
            return Admitted::Here(result.put("decision", "REFUSED"));
        }
        if create.timeout_seconds.is_some_and(|timeout| queued_seconds >= timeout as f64) {
            dequeue(root, &ticket);
            let ended = if reason(&result).as_deref() == Some("DEFER_NATIVE_QUIET") { "REFUSED" } else { "TIMED_OUT" };
            return Admitted::Here(result.put("decision", ended));
        }
        if offer && now_ms() - offered_at >= declined.retry_milliseconds {
            offered_at = now_ms();
            if let Some(code) = try_offload(Waiting { root, ticket: Some(&ticket), request, create, command, declined: &mut declined }, reason(&result).is_some_and(|reason| reason.starts_with("DEFER_")), host, terminations).await {
                dequeue(root, &ticket);
                return Admitted::Offloaded(code);
            }
        }
        if reason(&result) != reported {
            print_to(&result.clone().put("decision", "QUEUED"), true);
            reported = reason(&result);
        }
        if let Some(follow) = &follow {
            follow.follow(&ticket, queued_at);
        }
        tokio::select! {
            () = state_change(root, watch(reason(&result).as_deref())) => {}
            () = tokio::time::sleep(queue_time_left(create.timeout_seconds, queued_seconds)) => {}
            number = terminations.next() => {
                dequeue(root, &ticket);
                std::process::exit(128 + number);
            }
        }
    }
}

/// How long a queued request may still wait before its --timeout-seconds ends it; without one it waits on.
fn queue_time_left(timeout_seconds: Option<u64>, queued_seconds: f64) -> Duration {
    timeout_seconds.map_or(Duration::MAX, |timeout| Duration::from_secs_f64((timeout as f64 - queued_seconds).max(0.0)))
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

pub fn north_command() -> String {
    north_base::platform::north_binary::north_binary().to_string_lossy().into_owned()
}

/// The lease's own compiler-cache server socket: sccache forks every compile from its server, so one server shared by every lease ran each lease's compiles in the scope of whichever process started it, outside the lease's CPU quota and measurement. Named by the lease's first uuid group because a Unix socket path holds 107 bytes and the private desktop's runtime directory already spends 68 of them before the file name.
pub(super) fn cache_server_socket(root: &Path, lease: &str) -> std::path::PathBuf {
    root.join("sccache").join(format!("{}.sock", lease.split('-').next().unwrap_or(lease)))
}

pub(super) fn scope_command(root: &Path, request: &Request, admitted: &Obj, timeout_seconds: Option<u64>, command: &[String]) -> tokio::process::Command {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let north = north_command();
    let mut arguments: Vec<String> = ["--user", "--scope", "--quiet", "--collect", "--expand-environment=no"].map(str::to_owned).to_vec();
    if let (Some(runtime), Some(bus)) = (env("XDG_RUNTIME_DIR"), env("DBUS_SESSION_BUS_ADDRESS")) {
        arguments.extend([format!("--setenv=XDG_RUNTIME_DIR={runtime}"), format!("--setenv=DBUS_SESSION_BUS_ADDRESS={bus}")]);
    }
    arguments.extend([format!("--unit={unit}"), format!("--slice={}", lease_slice(request.admitted_class()))]);
    let scope = admitted.get("scope").cloned().unwrap_or(Value::Null);
    // A quota-throttled game client stalls for the rest of each period.
    if let Some(cpus) = field(&scope, "cpus").filter(|cpus| cpus.is_finite() && *cpus > 0.0 && request.name != "native" && !elastic_class(request.admitted_class())) {
        arguments.push(format!("--property=CPUQuota={}%", super::json::show(cpus * 100.0)));
    }
    arguments.extend([format!("--property=MemoryHigh={}M", super::json::show(field(&scope, "memoryMiB").unwrap_or(f64::NAN))), "--property=KillMode=control-group".to_owned()]);
    if FROZEN_CLASSES.contains(&request.name.as_str()) {
        arguments.extend(["--".to_owned(), north.clone(), "capacity".to_owned(), "deadline".to_owned(), exact_lease(root, &lease).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()]);
    } else {
        arguments.extend([format!("--property=RuntimeMaxSec={}", timeout_seconds.map_or_else(|| "infinity".to_owned(), |seconds| format!("{seconds}s"))), "--".to_owned()]);
    }
    let jobs = field(&scope, "cpus").filter(|cpus| cpus.is_finite()).map_or(1.0, f64::ceil).max(1.0).to_string();
    if request.name == "native" {
        arguments.extend(dev_shell::bare(&north, command));
    } else {
        let sized = north_base::nix_command::sized(command, &jobs);
        arguments.extend(dev_shell::enter(&north, sized.as_deref().unwrap_or(command)));
    }
    let mut child = if fleet_session() {
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
        child.env("CARGO_BUILD_JOBS", &jobs).env("RUST_TEST_THREADS", &jobs);
        let socket = cache_server_socket(root, &lease);
        if let Some(directory) = socket.parent() {
            let _ = std::fs::create_dir_all(directory);
        }
        child.env("SCCACHE_SERVER_UDS", socket);
    }
    requeue::forget(&mut child);
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
    let mut entry = Obj::new()
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
        .put_some("besideNative", request.beside_native.clone());
    if let Some(cost) = build_offload::ShipCost::read(env, &crate::platform::host::name()) {
        for (key, value) in cost.fields() {
            entry.set(key, value);
        }
    }
    entry
}

fn model_fields(entry: Obj, owner: &str, request: &Request, command: &[String], usage: &Usage, cache: &str, memory_high: Option<f64>) -> Obj {
    let predicted = |key: &str| request.forecast.as_ref().and_then(|forecast| forecast.get(key)).cloned().unwrap_or(Value::Null);
    entry
        .put("key", command_key(owner, command))
        .put("host", host_name())
        .put("cache", cache)
        .put("anonPeakMiB", super::json::opt(usage.anon_peak_mib.map(|anon| (anon + 0.5).floor())))
        .put("memoryStallUsec", super::json::opt(usage.memory_stall_usec))
        .put("censored", censored(usage.memory_stall_usec, usage.peak_memory_mib, memory_high))
        .put("predictedWallSeconds", predicted("wallSeconds"))
        .put("predictedWallP90Seconds", predicted("wallP90Seconds"))
        .put("predictedMemoryMiB", predicted("memoryMiB"))
        .put("predictionBasis", predicted("basis"))
}

pub(super) fn learn(db: &rusqlite::Connection, row: &Value) -> rusqlite::Result<()> {
    let subject = Subject {
        command: row.get("command").map_or_else(String::new, Value::to_string),
        key: text(row, "key").unwrap_or_default().to_owned(),
        shape: text(row, "shape").unwrap_or_default().to_owned(),
        host: text(row, "host").unwrap_or_default().to_owned(),
        cache: text(row, "cache").unwrap_or_default().to_owned(),
    };
    let censored = row.get("censored").and_then(Value::as_bool).unwrap_or(false);
    let observation = Observation {
        subject: &subject,
        wall_seconds: field(row, "wallSeconds").unwrap_or(f64::NAN),
        cpu_seconds: field(row, "cpuSeconds").unwrap_or(f64::NAN),
        anon_peak_mib: field(row, "anonPeakMiB").unwrap_or(f64::NAN),
        censored_at_mib: censored.then(|| field(row, "reservedMemoryMiB")).flatten(),
        failed: field(row, "exitCode") != Some(0.0),
    };
    update(db, &observation)
}

fn record(entry: &Obj, usage: &Usage, command: &[String], request: &Request, lease: &str) {
    let cwd = std::env::current_dir().map(|cwd| cwd.to_string_lossy().into_owned()).ok();
    let row = entry
        .clone()
        .put("ioReadBytes", super::json::opt(usage.io_read_bytes))
        .put("ioWriteBytes", super::json::opt(usage.io_write_bytes))
        .put("ioPressureSomeUsec", super::json::opt(usage.io_pressure_some_usec))
        .put("diskBusyShare", super::json::opt(usage.disk_busy_share));
    let view = row.value();
    let stored = super::database::capacity_database().and_then(|mut db| {
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        super::database::insert_capacity_run(&transaction, &format!("lease:{lease}"), &view, &task_signature(command, cwd.as_deref(), &request.name, true), cwd.as_deref())?;
        learn(&transaction, &view)?;
        transaction.commit()
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

/// The decision printed before RELEASED when a lease's command exits nonzero, so that exit is never a bare RESERVED/RELEASED pair: it names the exit, the program and the reason admission gave (a gaming hold that admits the class says RUN).
pub(super) fn command_exit(request: &Request, owner: &str, command: &[String], admitted: &Obj, exit_code: i32) -> Option<Obj> {
    (exit_code != 0).then(|| {
        Obj::new()
            .put("decision", "COMMAND_EXITED")
            .put("exitCode", exit_code)
            .put("program", command.first().map_or("", String::as_str))
            .put("class", request.name.as_str())
            .put("owner", owner)
            .put("admission", admitted.get("reason").cloned().unwrap_or(Value::Null))
            .put("lease", admitted.get("lease").cloned().unwrap_or(Value::Null))
    })
}

async fn run_admitted(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], admitted: &Obj, terminations: &mut Terminations) -> i32 {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let child_cpu_before = children_cpu_seconds();
    let cache = current_cache_state();
    let exit_code = match scope_command(root, request, admitted, timeout_seconds, command).spawn() {
        Ok(child) => {
            let measurement = measure_scope(root, &lease, scope_cgroup(request.admitted_class(), &unit));
            let exit_code = supervise(child, &unit, terminations).await;
            let usage = measurement.finish();
            let entry = usage_entry(owner, request, command, admitted, exit_code, &usage, children_cpu_seconds() - child_cpu_before);
            let memory_high = admitted.get("scope").and_then(|scope| field(scope, "memoryMiB"));
            let entry = model_fields(entry, owner, request, command, &usage, cache, memory_high);
            record(&entry, &usage, command, request, &lease);
            if let Some(line) = command_exit(request, owner, command, admitted, exit_code) {
                print_to(&line, true);
            }
            exit_code
        }
        Err(error) => {
            eprintln!("machine-capacity: {error}");
            1
        }
    };
    stop_scope(&unit);
    let _ = std::fs::remove_file(cache_server_socket(root, &lease));
    if scope_live(&lease) {
        eprintln!("machine-capacity: lease cleanup failed: scope still live; retained lease {lease}");
    } else {
        print_to(&change_lease(root, &lease, owner, None, true), true);
    }
    exit_code
}

fn refusal(admitted: &Obj) -> Option<String> {
    let view = admitted.value();
    if text(&view, "reason") == Some(NO_BUILD_HOST) {
        return Some(format!("refused: this host does not compile for fleet sessions and no build host took the job ({}); the detail names what to change or wait for", text(&view, "detail").unwrap_or_default()));
    }
    if text(&view, "reason") == Some("PAIR_HELD") {
        let holder = view.get("holder").map_or_else(String::new, |holder| holder.as_str().map_or_else(|| holder.to_string(), str::to_owned));
        return Some(format!("refused: pair {} is held by {holder}; free pair: {}", view["pair"], view["freePair"]));
    }
    if text(&view, "decision") == Some("TIMED_OUT") {
        return Some(format!("queued {}s, past its --timeout-seconds, never admitted: {}", view["queuedSeconds"], text(&view, "reason").unwrap_or("unknown")));
    }
    (text(&view, "decision") == Some("REFUSED")).then(|| {
        format!("refused after {}s: native evidence run {} is live; retry after it ends", view["queuedSeconds"], quiet_owners(&view).join(", "))
    })
}

pub fn run_scoped(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], host: &Host) -> u8 {
    if request.name == "agent" {
        fail("run --class must be moderate, heavy, exclusive, gpu, critical, tom, runtime, or native", 2);
    }
    if let Some(parent) = batch_class(&request.name).then(|| nested_parent(root)).flatten() {
        print_to(&Obj::new().put("decision", "NESTED").put("parent", parent.as_str()).put("class", request.name.as_str()).put("owner", owner), true);
        let mut arguments = vec![exact_lease(root, &parent).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()];
        arguments.extend(dev_shell::enter(&north_command(), command));
        return crate::capacity::deadline::run(&arguments);
    }
    // The parent limit and weights must exist before any admitted command executes.
    let signals = read_signals();
    if !apply_profile(active_profile(root).1, signals.cores, signals.slice_cpus) {
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
        super::priority::record_use(owner, request, &admitted, command);
        u8::try_from(run_admitted(root, request, owner, timeout_seconds, command, &admitted, &mut terminations).await & 0xff).unwrap_or(1)
    })
}
