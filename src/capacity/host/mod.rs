//! `north capacity host`: the machine-capacity helper's leases, queue, admission and scoped runs.

use std::path::Path;

use serde_json::Value;

use super::Host;
use json::{Obj, field, num};

mod args;
mod database;
mod decision;
mod estimate;
mod gpu;
mod json;
mod machine;
mod measure;
mod policy;
mod request;
mod run;
mod shape;
mod sizing;
mod state;
mod status;
mod systemd;

const USAGE: &str = "usage: estimate [--json] [--class CLASS] [--owner OWNER] -- CMD ARG...|status|probe|mode|reserve|renew|release|run|session [--new-clients N] [--new-desktops N] [--wait-seconds N] [--quiet [--quiet-seconds N]] [--alone] [--pair K] [--beside-native NATIVE-OWNER]; see machine-capacity";

pub(crate) fn fail(message: &str, status: i32) -> ! {
    eprintln!("machine-capacity: {message}");
    std::process::exit(status)
}

pub(crate) fn print_to(result: &Obj, stderr: bool) {
    if stderr {
        eprintln!("{}", result.text());
    } else {
        println!("{}", result.text());
    }
}

/// Renews or releases an agent lease, or settles a finished run's lease.
pub(crate) fn change_lease(root: &Path, id: &str, owner: &str, timeout_seconds: Option<u64>, settled_run: bool) -> Obj {
    state::with_lock(root, || {
        let path = state::exact_lease(root, id);
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
            state::remove_lease(&path);
            return Obj::new().put("decision", "RELEASED").put("lease", id);
        };
        if field(&lease, "expiresAt").is_none_or(|expires| expires <= machine::now_ms()) {
            let _ = std::fs::remove_file(&path);
            fail(&format!("lease already expired: {id}"), 75);
        }
        let expires_at = num(machine::now_ms() + timeout_seconds as f64 * 1000.0);
        let renewed = Obj::new().spread(&lease).put("expiresAt", expires_at.clone());
        state::write_lease(&path, &renewed, false);
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

const FIXTURE_OPTIONS: [&str; 26] = [
    "--profile", "--class", "--cores", "--memory-total-mib", "--memory-available-mib", "--protected-cpu-some-avg10-basis-points", "--memory-full-avg10-basis-points",
    "--leased-cpus", "--leased-native-cpus", "--leased-memory-mib", "--peer-batch-runs", "--peer-exclusive-runs", "--unbounded-runs", "--memory-gib",
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
    let granted = request::grant(profile, cores, memory_total, &request);
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
        .put("aggregateCpuLimit", num(policy::aggregate_cpus(profile, cores)));
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
    let cores = machine::read_signals().cores;
    print_to(&Obj::new().put("mode", mode).put("profile", profile).put("aggregateCpuLimit", num(policy::aggregate_cpus(profile, cores))), false);
    0
}

fn probe(root: &Path, argv: &[String], host: &Host) -> u8 {
    let options = args::Options::parse(argv, 1, &["--class", "--memory-gib", "--beside-native", "--new-clients", "--new-desktops", "--pair", "--owner"], &[]);
    if options.separator != argv.len() {
        fail("probe accepts no command", 2);
    }
    let signals = machine::read_signals();
    let beside_native = options.has("--beside-native").then(|| options.owner("--beside-native"));
    let owner = options.has("--owner").then(|| options.owner("--owner"));
    let request = request::Request { beside_native, owner, ..options.class(signals.cores) };
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

const RUN_OPTIONS: [&str; 12] = ["--class", "--owner", "--timeout-seconds", "--memory-gib", "--beside-native", "--new-clients", "--new-desktops", "--quiet", "--wait-seconds", "--quiet-seconds", "--pair", "--alone"];

fn requested(options: &args::Options, operation: &str, owner: &str, command: &[String], cores: f64) -> request::Request {
    let mut request = if options.has("--memory-gib") {
        options.class(cores)
    } else if options.has("--class") {
        sizing::charge_from_usage(options.class(cores), owner, command, cores)
    } else {
        sizing::size_from_usage(command, owner, cores)
    };
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
    request.landing = operation == "run" && owner == "land-check" && machine::env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty()) && policy::batch_class(&request.name);
    request
}

fn run_or_session(root: &Path, argv: &[String], host: &Host) -> u8 {
    let operation = argv[0].as_str();
    let options = args::Options::parse(argv, 1, &RUN_OPTIONS, &["--quiet", "--alone"]);
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

pub(crate) fn run(argv: &[String], host: &Host) -> u8 {
    let operation = argv.first().map_or("", String::as_str);
    match operation {
        "estimate" => return estimate(argv),
        "fixture" => return fixture(argv, host),
        _ => {}
    }
    let root = state::runtime_root();
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
        "probe" => probe(&root, argv, host),
        "reserve" => reserve(&root, argv, host),
        "renew" | "release" => renew_or_release(&root, argv),
        "run" | "session" => run_or_session(&root, argv, host),
        _ => fail(USAGE, 2),
    }
}
