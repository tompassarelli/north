//! `north capacity host`: the machine-capacity helper's leases, queue, admission and scoped runs.

use std::path::Path;

use serde_json::Value;

use super::effects::Host;
use super::cpus::aggregate_cpus;
use super::lease_lifecycle::Phase;
use super::local_machine::{fail, runtime_root};
use json::{Obj, field, num};

mod actions_pull;
mod admissions;
pub use admissions::ledger_rows as admission_ledger_rows;
mod args;
mod database;
pub use database::{QUEUED_BOUND_SECONDS, Queued, capacity_database_readonly, queued_admissions, queued_behind_capacity};
mod decision;
mod estimate;
mod learned;
#[cfg(test)]
mod estimate_replay_tests;
mod fleet_pull;
mod gpu;
mod handover;
mod offer_lifecycle;
mod offering;
mod terminations;
#[cfg(test)]
mod handover_tests;
#[cfg(test)]
mod offering_tests;
#[cfg(test)]
mod gpu_tests;
mod json;
mod machine;
pub(super) use machine::host_cores;
#[cfg(test)]
mod landing_tests;
#[cfg(test)]
mod machine_tests;
mod measure;
mod offload_cost;
mod model;
mod calibration;
pub use calibration::{calibration_command, model_command};
pub(super) mod policy;
#[cfg(test)]
mod policy_tests;
mod priority;
mod quiet_holds;
mod request;
mod requeue;
mod run;
#[cfg(test)]
mod lease_lifecycle_tests;
#[cfg(test)]
mod run_tests;
mod shape;
mod sizing;
mod state;
mod status;
mod systemd;
mod unleased_native;
#[cfg(test)]
mod unleased_native_tests;

const USAGE: &str = "usage: offload-cost [--last N]|admit-job|estimate [--json] [--class CLASS] [--owner OWNER] -- CMD ARG...|status|backlog|land-waiting|hand-over [--wait-seconds N]|probe|mode|reserve|renew|release|run|session [--new-clients N] [--new-desktops N] [--wait-seconds N] [--quiet [--quiet-seconds N]] [--latency] [--alone] [--pair K] [--beside-native NATIVE-OWNER] [--priority tom-ask]; see machine-capacity";

pub use actions_pull::run as actions_pull;
pub use fleet_pull::run as pull;

/// The cores a land check asks of a check host: its highest measured peak once enough newest land-check runs all peaked under the heavy class, else `land_offload_cpus`.
pub fn land_check_cpus() -> f64 {
    land_check_charge(admission_default("land_offload_cpus"), &database::recent_usage(5000, |entry| entry["owner"].as_str() == Some("land-check") && entry["class"].as_str() == Some("heavy") && entry["wallSeconds"].as_f64().is_some_and(|wall| wall > 0.0)))
}

/// The MiB one offloaded job peaks at on this host: the 90th percentile of the newest measured lander runs' peaks once `heavy_peak_charge_runs` of them are recorded, else `dispatch_heavy_memory_mib`.
pub fn job_peak_mib() -> f64 {
    let runs = database::recent_usage(5000, |entry| matches!(entry["owner"].as_str(), Some("land-check" | "land-offload")) && entry["class"].as_str() == Some("heavy") && entry["wallSeconds"].as_f64().is_some_and(|wall| wall > 0.0));
    let mut peaks: Vec<f64> = runs.iter().filter_map(|run| run["peakMemoryMiB"].as_f64()).collect();
    if (peaks.len() as f64) < policy::default_number("heavy_peak_charge_runs") {
        return crate::capacity::admission::dispatch_request_mib(crate::capacity::admission::Class::Heavy);
    }
    peaks.sort_by(f64::total_cmp);
    peaks[(peaks.len() * 9 / 10).min(peaks.len() - 1)]
}

/// Why this host starts no other offloaded job now, read from its whole-host CPU pressure and available memory; None when it starts one.
pub fn job_refusal() -> Option<String> {
    let signals = machine::read_signals();
    let load = crate::capacity::job_admission::HostLoad { cpu_some_percent: signals.host_cpu_some / 100.0, memory_total_mib: signals.memory_total_mib, memory_available_mib: signals.memory_available_mib };
    crate::capacity::job_admission::refusal(&load, job_peak_mib())
}

/// `admit-job`: exits 0 when this host starts another offloaded job now, else prints why and exits 75, the answer the slot script and the slot warmer act on.
fn admit_job(argv: &[String]) -> u8 {
    if argv.len() != 1 {
        fail("admit-job accepts no arguments", 2);
    }
    match job_refusal() {
        None => 0,
        Some(why) => {
            println!("{why}");
            75
        }
    }
}

pub(super) fn land_check_charge(declared: f64, runs: &[Value]) -> f64 {
    let newest = &runs[runs.len().saturating_sub(2 * policy::default_number("heavy_peak_charge_runs") as usize)..];
    let peaks: Vec<f64> = newest.iter().map(|run| run["peakCores"].as_f64().unwrap_or(f64::NAN)).collect();
    policy::heavy_peak_charge(declared, &peaks).unwrap_or(declared)
}

fn admission_default(name: &str) -> f64 {
    crate::capacity::admission::default_mib(name)
}

pub fn print_to(result: &Obj, stderr: bool) {
    if stderr {
        eprintln!("{}", result.text());
    } else {
        println!("{}", result.text());
    }
}

/// Renews or releases an agent lease, or settles a finished run's lease.
pub fn change_lease(root: &Path, id: &str, owner: &str, timeout_seconds: Option<u64>, settled_run: bool) -> Obj {
    state::with_lock(root, || {
        let path = state::exact_lease(root, id);
        let mut ledger = state::ledger(root);
        if !path.exists() {
            if settled_run {
                return Obj::new().put("decision", "RECLAIMED").put("lease", id);
            }
            fail(&format!("unknown or expired lease: {id}"), 75);
        }
        let lease: Value = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_else(|| fail(&format!("malformed helper-owned lease: {}", path.display()), 1));
        if json::text(&lease, "owner") != Some(owner) {
            fail(&format!("lease owner mismatch: {id}"), 2);
        }
        if request::is_run(&lease) && !settled_run {
            fail("run leases belong to their foreground wrapper", 2);
        }
        let Some(timeout_seconds) = timeout_seconds else {
            let cause = if settled_run { "the wrapper settled its run" } else { "its holder released it" };
            state::end_lease(root, &mut ledger, id, Phase::Released, cause);
            return Obj::new().put("decision", "RELEASED").put("lease", id);
        };
        if field(&lease, "expiresAt").is_none_or(|expires| expires <= machine::now_ms()) {
            state::end_lease(root, &mut ledger, id, Phase::Reclaimed, "its holder renewed it after its expiry");
            fail(&format!("lease already expired: {id}"), 75);
        }
        let expires_at = num(machine::now_ms() + timeout_seconds as f64 * 1000.0);
        let renewed = Obj::new().spread(&lease).put("expiresAt", expires_at.clone());
        state::write_lease(&path, &renewed, false);
        if ledger.phase(id).ok().flatten() == Some(Phase::Reserved) {
            state::advance(&mut ledger, id, Phase::Reserved, "its holder renewed it", expires_at.as_f64());
        }
        Obj::new().put("decision", "RENEWED").put("lease", id).put("expiresAt", expires_at)
    })
}

fn known(value: &Value) -> String {
    match value {
        Value::Null => "unknown".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn estimate(argv: &[String]) -> u8 {
    let json = argv.get(1).map(String::as_str) == Some("--json");
    let options = args::Options::parse(argv, if json { 2 } else { 1 }, &["--class", "--owner"], &[]);
    let command = argv.get(options.separator + 1..).unwrap_or_default();
    if options.separator == argv.len() || command.is_empty() {
        fail("estimate requires -- COMMAND ARG...", 2);
    }
    let kind = options.get("--class").unwrap_or("moderate");
    if !policy::CLASS_NAMES.contains(&kind) {
        fail("unknown resource class", 2);
    }
    let owner = options.has("--owner").then(|| options.owner("--owner"));
    let estimate = sizing::estimate_command(command, kind, owner.as_deref(), machine::host_cores());
    if json {
        println!("{}", json::js_numbers(estimate));
        return 0;
    }
    let at = |pointer: &str| known(estimate.pointer(pointer).unwrap_or(&Value::Null));
    let signature = &estimate["signature"];
    println!("{} repo={} package={} class={}", known(&signature["family"]), known(&signature["repo"]), signature["package"].as_str().unwrap_or("none"), known(&estimate["class"]));
    println!("cores p50={} p90={}; peak memory MiB p50={} p90={}", at("/cores/p50"), at("/cores/p90"), at("/peakMemoryMiB/p50"), at("/peakMemoryMiB/p90"));
    println!(
        "I/O read bytes p50={} p90={}; write bytes p50={} p90={}; disk busy share p50={} p90={} ({})",
        at("/ioReadBytes/p50"), at("/ioReadBytes/p90"), at("/ioWriteBytes/p50"), at("/ioWriteBytes/p90"), at("/diskBusyShare/p50"), at("/diskBusyShare/p90"), at("/diskBusyBasis")
    );
    println!("samples={} match={} confidence={} I/O samples={}", at("/sampleCount"), at("/matchLevel"), at("/confidence"), at("/ioSampleCount"));
    0
}

const FIXTURE_OPTIONS: [&str; 27] = [
    "--profile", "--class", "--cores", "--memory-total-mib", "--memory-available-mib", "--protected-cpu-some-avg10-basis-points", "--memory-full-avg10-basis-points",
    "--leased-cpus", "--leased-native-cpus", "--leased-memory-mib", "--peer-batch-runs", "--peer-exclusive-runs", "--unbounded-runs", "--memory-gib", "--cpus",
    "--cpu-some-avg10-basis-points", "--queued-ahead", "--exclusive-waiting", "--gpu-busy-percent", "--gpu-clients", "--new-clients", "--new-desktops",
    "--gpu-runs", "--native-waiting", "--quiet-native-runs", "--beside-native", "--run-queue-load",
];

const FIXTURE_COUNTS: [(&str, &str); 15] = [
    ("leasedNativeCpus", "--leased-native-cpus"), ("cpuSomeAvg10BasisPoints", "--cpu-some-avg10-basis-points"), ("peerBatchRuns", "--peer-batch-runs"),
    ("peerExclusiveRuns", "--peer-exclusive-runs"), ("unboundedRuns", "--unbounded-runs"), ("queuedAhead", "--queued-ahead"), ("exclusiveWaiting", "--exclusive-waiting"),
    ("gpuBusyPercent", "--gpu-busy-percent"), ("gpuClients", "--gpu-clients"), ("newClients", "--new-clients"), ("newDesktops", "--new-desktops"),
    ("gpuRuns", "--gpu-runs"), ("nativeWaiting", "--native-waiting"), ("quietNativeRuns", "--quiet-native-runs"), ("runQueueLoad", "--run-queue-load"),
];

/// Admission on stated facts, without reading the machine.
fn fixture(argv: &[String], host: &Host) -> u8 {
    let options = args::Options::parse(argv, 1, &FIXTURE_OPTIONS, &[]);
    if options.separator != argv.len() {
        fail("fixture accepts no command", 2);
    }
    let cores = args::positive_integer(options.required("--cores"), "--cores", None) as f64;
    let request = options.class(cores);
    let memory_full = args::nonnegative_integer(options.required("--memory-full-avg10-basis-points"), "--memory-full-avg10-basis-points") as f64 / 100.0;
    let leased_cpus = json::number(options.required("--leased-cpus"));
    if !leased_cpus.is_finite() || leased_cpus < 0.0 {
        fail("--leased-cpus requires nonnegative finite cores", 2);
    }
    let profile = options.required("--profile");
    if profile != "attended" && profile != "unattended" {
        fail("--profile must be attended or unattended", 2);
    }
    let memory_total = args::positive_integer(options.required("--memory-total-mib"), "--memory-total-mib", None) as f64;
    let granted = request::grant(profile, cores, None, memory_total, &request);
    let mut facts = Obj::new()
        .put("profile", profile)
        .put("class", request.name.as_str())
        .put("cores", num(cores))
        .put("memoryTotalMiB", num(memory_total))
        .put("requestedCpus", num(granted.charged.cpus))
        .put("requestedMemoryMiB", num(granted.charged.memory_mib))
        .put("memoryAvailableMiB", args::positive_integer(options.required("--memory-available-mib"), "--memory-available-mib", None))
        .put("protectedCpuSomeAvg10BasisPoints", args::nonnegative_integer(options.required("--protected-cpu-some-avg10-basis-points"), "--protected-cpu-some-avg10-basis-points"))
        .put("leasedMemoryMiB", args::nonnegative_integer(options.required("--leased-memory-mib"), "--leased-memory-mib"))
        .put("leasedBatchCpus", num(leased_cpus));
    for (key, option) in FIXTURE_COUNTS {
        facts.set(key, args::nonnegative_integer(options.get(option).unwrap_or("0"), option));
    }
    facts.set("besideNative", options.has("--beside-native"));
    let (code, _) = decision::admit(facts, host);
    let result = Obj::new()
        .put("decision", code.as_str())
        .put("profile", profile)
        .put("class", request.name.as_str())
        .put("cpus", num(granted.charged.cpus))
        .put("memoryMiB", num(granted.charged.memory_mib))
        .put_some("clamped", granted.clamped)
        .put("memoryFullAvg10", num(memory_full))
        .put("leasedCpuCeilings", num(leased_cpus))
        .put("aggregateCpuLimit", num(aggregate_cpus(profile, cores, None)));
    print_to(&result, false);
    if code == "RUN" { 0 } else { 75 }
}

fn mode(root: &Path, argv: &[String]) -> u8 {
    if argv.len() > 2 {
        fail("usage: mode [away|present|auto]", 2);
    }
    if let Some(mode) = argv.get(1) {
        if !state::known_mode(mode) {
            fail("mode must be away, present, or auto", 2);
        }
        state::ensure_state(root);
        let path = root.join("mode");
        state::write_private(&path, &format!("{mode}\n"), false).unwrap_or_else(|error| fail(&format!("{}: {error}", path.display()), 1));
    }
    let (mode, profile) = state::active_profile(root);
    if !state::ensure_profile(root, true) {
        fail("cannot apply capacity profile", 75);
    }
    let signals = machine::read_signals();
    print_to(&Obj::new().put("mode", mode).put("profile", profile).put("aggregateCpuLimit", num(aggregate_cpus(profile, signals.cores, signals.slice_cpus))), false);
    0
}

fn probe(root: &Path, argv: &[String], host: &Host) -> u8 {
    let options = args::Options::parse(argv, 1, &["--class", "--memory-gib", "--cpus", "--beside-native", "--new-clients", "--new-desktops", "--pair", "--owner"], &[]);
    if options.separator != argv.len() {
        fail("probe accepts no command", 2);
    }
    let signals = machine::read_signals();
    let beside_native = options.has("--beside-native").then(|| options.owner("--beside-native"));
    let owner = options.has("--owner").then(|| options.owner("--owner"));
    let request = request::Request { beside_native, owner, ..options.class(signals.cores) };
    if request.name == "tom" && let Some(message) = priority::tom_class_refusal(request.owner.as_deref()) {
        fail(&message, 2);
    }
    let result = decision::decision(root, &request, None, None, host);
    print_to(&result, false);
    if result.get("decision").and_then(Value::as_str) == Some("RUN") { 0 } else { 75 }
}

fn reserve(root: &Path, argv: &[String], host: &Host) -> u8 {
    let options = args::Options::parse(argv, 1, &["--class", "--owner", "--timeout-seconds"], &[]);
    if options.separator != argv.len() {
        fail("reserve accepts no command", 2);
    }
    if options.required("--class") != "agent" {
        fail("reserve --class must be agent", 2);
    }
    let signals = machine::read_signals();
    let request = options.class(signals.cores);
    let owner = options.owner("--owner");
    let timeout_seconds = args::positive_integer(options.required("--timeout-seconds"), "--timeout-seconds", Some(policy::maximum_seconds("agent")));
    let create = request::Create { kind: "agent", owner, timeout_seconds: Some(timeout_seconds) };
    let result = decision::decision(root, &request, Some(&create), None, host);
    admissions::record(&create.owner, &request, &result);
    print_to(&result, false);
    if result.get("decision").and_then(Value::as_str) == Some("RESERVED") { 0 } else { 75 }
}

fn renew_or_release(root: &Path, argv: &[String]) -> u8 {
    let renew = argv[0] == "renew";
    let allowed: &[&str] = if renew { &["--lease", "--owner", "--timeout-seconds"] } else { &["--lease", "--owner"] };
    let options = args::Options::parse(argv, 1, allowed, &[]);
    if options.separator != argv.len() {
        fail(&format!("{} accepts no command", argv[0]), 2);
    }
    let timeout_seconds = renew.then(|| args::positive_integer(options.required("--timeout-seconds"), "--timeout-seconds", Some(policy::maximum_seconds("agent"))));
    print_to(&change_lease(root, options.required("--lease"), &options.owner("--owner"), timeout_seconds, false), false);
    0
}

const LAND_CPUS: &str = "SAFE_PUSH_LAND_CPUS";

const RUN_OPTIONS: [&str; 15] = [
    "--class", "--owner", "--timeout-seconds", "--memory-gib", "--cpus", "--beside-native", "--new-clients", "--new-desktops", "--quiet", "--wait-seconds", "--quiet-seconds", "--pair", "--alone", "--latency", "--priority",
];

fn landing_run(operation: &str, owner: &str, class: &str, land_head: Option<&str>, landing: Option<&str>) -> bool {
    operation == "run" && policy::batch_class(class) && (lander_owner(owner) || (owner.starts_with("gate:") && land_head.is_some_and(|head| !head.is_empty()) && landing == Some("1")))
}

/// The owners of the lander's own runs: the land checks and their post-land runs, the offload warm build, and the gates of its worktrees (`lander`, `lander-N`, `lander-probe-N`, `lander-replay`, `lander-replay-N`, `lander-goals`), which carry no session and so take the worktree's name.
fn lander_owner(owner: &str) -> bool {
    let Some(worktree) = owner.strip_prefix("gate:") else { return matches!(owner, "land-check" | "land-offload") };
    let numbered = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    match worktree.strip_prefix("lander") {
        Some("" | "-replay" | "-goals") => true,
        Some(rest) => rest.strip_prefix('-').is_some_and(|rest| numbered(rest) || rest.strip_prefix("probe-").or_else(|| rest.strip_prefix("replay-")).is_some_and(numbered)),
        None => false,
    }
}

fn size_landing(request: &mut request::Request, cores: f64, cap: Option<f64>, charge: impl FnOnce() -> f64) {
    if request.landing {
        let ceiling = policy::landing_check(cores);
        let share = cap.filter(|cap| cap.is_finite() && *cap >= 1.0).map_or(ceiling.cpus, f64::ceil);
        request.resources = Some(policy::Resources { cpus: ceiling.cpus.min(share).min(charge()), ..request.resources() });
        request.declared = Some(ceiling);
    }
}

fn requested(options: &args::Options, operation: &str, owner: &str, command: &[String], cores: f64) -> request::Request {
    if options.get("--class") == Some("tom") && let Some(message) = priority::tom_class_refusal(Some(owner)) {
        fail(&message, 2);
    }
    let mut request = if options.has("--memory-gib") || options.has("--cpus") {
        options.class(cores)
    } else if options.has("--class") {
        sizing::charge_from_usage(options.class(cores), owner, command, cores)
    } else {
        sizing::size_from_usage(command, owner, cores)
    };
    sizing::attach_forecast(&mut request, owner, command);
    if request.estimate.is_none() {
        request.estimate = Some(sizing::estimate_command(command, &request.name, Some(owner), cores));
    }
    request.beside_native = options.has("--beside-native").then(|| options.owner("--beside-native"));
    let native = request.name == "native";
    if options.has("--quiet") {
        if !native {
            fail("--quiet marks a native evidence session", 2);
        }
        request.quiet = true;
    }
    request.quiet_seconds = options.get("--quiet-seconds").map(|text| args::positive_integer(text, "--quiet-seconds", None));
    if options.has("--alone") {
        if !native {
            fail("--alone marks a native request that needs no other Warcraft client", 2);
        }
        request.alone = true;
    }
    if options.has("--latency") {
        if !native {
            fail("--latency marks a native session whose clients measure or serve frame-timed play", 2);
        }
        request.latency = true;
    }
    request.landing = landing_run(operation, owner, &request.name, machine::env("SAFE_PUSH_LAND_HEAD").as_deref(), machine::env("SAFE_PUSH_LANDING").as_deref());
    size_landing(&mut request, cores, machine::env(LAND_CPUS).and_then(|text| text.parse().ok()), land_check_cpus);
    if let Some(asked) = options.get("--priority") {
        if let Some(message) = priority::priority_refusal(asked, &request.name) {
            fail(&message, 2);
        }
        request.priority = (!request.landing).then_some(priority::TOM_ASK);
    }
    request
}

fn run_or_session(root: &Path, argv: &[String], host: &Host) -> u8 {
    let operation = argv[0].as_str();
    let options = args::Options::parse(argv, 1, &RUN_OPTIONS, &["--quiet", "--alone", "--latency"]);
    let command = argv.get(options.separator + 1..).unwrap_or_default();
    if options.separator == argv.len() || command.is_empty() {
        fail(&format!("{operation} requires -- COMMAND ARG..."), 2);
    }
    let signals = machine::read_signals();
    if !options.has("--class") && operation == "session" {
        fail("session requires --class", 2);
    }
    let owner = options.owner("--owner");
    let request = requested(&options, operation, &owner, command, signals.cores);
    // A session without one takes its class's default deadline; native sessions have none.
    let timeout_seconds = if options.has("--timeout-seconds") || operation == "run" {
        Some(args::positive_integer(options.required("--timeout-seconds"), "--timeout-seconds", Some(policy::maximum_seconds(&request.name))))
    } else {
        Some(policy::session_seconds(&request.name)).filter(|seconds| *seconds > 0)
    };
    run::run_scoped(root, &request, &owner, timeout_seconds, command, host)
}

/// Asks the outside broker to run this admission: a worker whose sandbox hides the user manager's sockets cannot put the command in a scope itself, and the broker starts the command in a new worker domain.
fn through_broker(argv: &[String]) -> u8 {
    use std::os::unix::process::CommandExt;
    let north = run::north_command();
    let error = std::process::Command::new(&north).args(["--worker-sandbox", "--outside", "--"]).arg(&north).args(["capacity", "host"]).args(argv).exec();
    eprintln!("machine-capacity: {north} --worker-sandbox --outside: {error}");
    126
}

pub fn run(argv: &[String], host: &Host) -> u8 {
    let operation = argv.first().map_or("", String::as_str);
    match operation {
        "estimate" => return estimate(argv),
        "fixture" => return fixture(argv, host),
        "offload-cost" => return offload_cost::print(argv),
        "admit-job" => return admit_job(argv),
        _ => {}
    }
    if operation == "run" && std::env::var_os(north_base::platform::session_scope::SOCKETS_HIDDEN).is_some() {
        return through_broker(argv);
    }
    let root = runtime_root();
    let user_runtime = format!("/run/user/{}/", machine::uid());
    if operation != "mode" && (machine::env("XDG_RUNTIME_DIR").is_none() || root.to_string_lossy().starts_with(&user_runtime)) {
        state::ensure_profile(&root, false);
    }
    match operation {
        "mode" => mode(&root, argv),
        "status" if argv.len() == 1 => {
            print_to(&status::status(&root), false);
            0
        }
        "status" => fail("status accepts no arguments", 2),
        "backlog" if argv.len() == 1 => {
            println!("{}", status::backlog(&root));
            0
        }
        "land-waiting" if argv.len() == 1 => {
            state::claim_land_check(&root);
            0
        }
        "hand-over" => handover::run(&root, argv),
        "probe" => probe(&root, argv, host),
        "reserve" => reserve(&root, argv, host),
        "renew" | "release" => renew_or_release(&root, argv),
        "run" | "session" => run_or_session(&root, argv, host),
        _ => fail(USAGE, 2),
    }
}
