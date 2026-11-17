//! What a finished lease leaves behind: its usage row, the estimate it teaches, and its release.

use std::path::Path;

use serde_json::Value;

use super::super::json::{Obj, field, num, opt, round, round2, text};
use super::super::machine::{env, now_ms, scope_cgroup};
use super::super::measure::{Usage, measure_scope};
use super::super::usage_log::{self, censored, command_key, current_cache_state};
use super::super::overrun::{Overrun, Usual};
use super::super::request::Request;
use super::super::shape::{command_shape, recorded_command};
use super::super::state::{advance, ledger};
use super::super::systemd::{scope_live, scope_unit, stop_scope};
use super::super::terminations::Terminations;
use super::super::{change_lease, print_to};
use crate::capacity::build_offload;
use crate::capacity::local_machine::host_name;
use super::scope::{cache_server_socket, children_cpu_seconds, scope_command, supervise};
use crate::capacity::lease_lifecycle::Phase;
use crate::capacity::subject_start::{self, Exit};
use north_foundation::foundation::exit::NO_VERDICT_STATUS;

fn usage_entry(owner: &str, request: &Request, command: &[String], admitted: &Obj, exit_code: i32, usage: &Usage, child_cpu: f64) -> Obj {
    let cpu_seconds = usage.cpu_seconds.max(child_cpu);
    let mean = cpu_seconds / usage.wall_seconds.max(0.001);
    let scope = admitted.get("scope").cloned().unwrap_or(Value::Null);
    let get = |key: &str| admitted.get(key).cloned().unwrap_or(Value::Null);
    let mut entry = Obj::new()
        .put("at", north_foundation::foundation::clock::Timestamp::now().iso_millis())
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
    if let Some(cost) = build_offload::ShipCost::read(env, &north_base::platform::host::name()) {
        for (key, value) in cost.fields() {
            entry.set(key, value);
        }
    }
    entry
}

fn model_fields(entry: Obj, owner: &str, command: &[String], usage: &Usage, cache: &str, memory_high: Option<f64>) -> Obj {
    entry
        .put("key", command_key(owner, command))
        .put("host", host_name())
        .put("cache", cache)
        .put("anonPeakMiB", opt(usage.anon_peak_mib.map(|anon| (anon + 0.5).floor())))
        .put("memoryStallUsec", opt(usage.memory_stall_usec))
        .put("censored", censored(usage.memory_stall_usec, usage.peak_memory_mib, memory_high))
}

fn record(entry: &Obj, usage: &Usage, request: &Request) {
    let row = entry
        .clone()
        .put_some("cwd", std::env::current_dir().ok().map(|cwd| cwd.to_string_lossy().into_owned()))
        .put("ioReadBytes", opt(usage.io_read_bytes))
        .put("ioWriteBytes", opt(usage.io_write_bytes))
        .put("ioPressureSomeUsec", opt(usage.io_pressure_some_usec))
        .put("diskBusyShare", opt(usage.disk_busy_share));
    if let Err(error) = usage_log::record_usage(&row.text()) {
        eprintln!("machine-capacity: usage log failed: {error}");
    }
    if request.lander && let Err(error) = super::super::sizing::record_landing_run(&entry.text()) {
        eprintln!("machine-capacity: lander run record failed: {error}");
    }
}

/// The decision printed before RELEASED when a lease's command exits nonzero, so that exit is never a bare RESERVED/RELEASED pair: it names the exit, the program and the reason admission gave (a gaming hold that admits the class says RUN). A lease whose subject never started says SUBJECT_NOT_STARTED with its launcher's exit, and exits no verdict.
pub(crate) fn command_exit(request: &Request, owner: &str, command: &[String], admitted: &Obj, exit: Exit) -> Option<Obj> {
    let (decision, exit_code) = match exit {
        Exit::Subject(code) => ("COMMAND_EXITED", code),
        Exit::NotStarted { launcher } => ("SUBJECT_NOT_STARTED", launcher),
    };
    (exit_code != 0).then(|| {
        Obj::new()
            .put("decision", decision)
            .put("exitCode", exit.code())
            .put_some("launcherExitCode", matches!(exit, Exit::NotStarted { .. }).then_some(exit_code))
            .put("program", command.first().map_or("", String::as_str))
            .put("class", request.name.as_str())
            .put("owner", owner)
            .put("admission", admitted.get("reason").cloned().unwrap_or(Value::Null))
            .put("lease", admitted.get("lease").cloned().unwrap_or(Value::Null))
    })
}

pub(super) async fn run_admitted(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], admitted: &Obj, terminations: &mut Terminations) -> i32 {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let child_cpu_before = children_cpu_seconds();
    let cache = current_cache_state();
    let witness = subject_start::witness(root, &lease);
    subject_start::arm(&witness);
    let subject = request.subject.as_deref().unwrap_or(command);
    let launched = now_ms() / 1000.0;
    let mut start = subject_start::Start::default();
    let mut cpu = None;
    let exit_code = match scope_command(root, request, admitted, timeout_seconds, command).spawn() {
        Ok(child) => {
            let measurement = measure_scope(root, &lease, scope_cgroup(request.scope_class(), &unit));
            let usual = Usual::of(request.estimate.as_ref().map(|estimate| &estimate["duration"]), timeout_seconds);
            let overrun = Overrun::watch(env(north_base::platform::session_scope::SESSION), subject, usual);
            let code = supervise(child, &unit, terminations, overrun).await;
            start = subject_start::start(&witness, launched, now_ms() / 1000.0);
            let exit = Exit::judge(code, &witness);
            let exit_code = exit.code();
            let usage = measurement.finish();
            let child_cpu = children_cpu_seconds() - child_cpu_before;
            cpu = Some(round2(usage.cpu_seconds.max(child_cpu)));
            let entry = usage_entry(owner, request, subject, admitted, exit_code, &usage, child_cpu);
            let memory_high = admitted.get("scope").and_then(|scope| field(scope, "memoryMiB"));
            let entry = model_fields(entry, owner, subject, &usage, cache, memory_high);
            record(&entry, &usage, request);
            if let Some(line) = command_exit(request, owner, command, admitted, exit) {
                print_to(&line, true);
            }
            exit_code
        }
        Err(error) => {
            eprintln!("machine-capacity: the lease's scope did not start, so its command never ran and reached no verdict: {error}");
            NO_VERDICT_STATUS
        }
    };
    let mut leases = ledger(root);
    if !leases.phase(&lease).ok().flatten().is_some_and(Phase::ended) {
        advance(&mut leases, &lease, Phase::Settling, &format!("the command exited with {exit_code}; its scope is being stopped"), None);
    }
    stop_scope(&unit);
    let _ = std::fs::remove_file(cache_server_socket(root, &lease));
    if scope_live(&lease) {
        eprintln!("machine-capacity: lease cleanup failed: scope still live; retained lease {lease}");
    } else {
        print_to(&started(change_lease(root, &lease, owner, None, true).put_some("cpuSeconds", cpu.map(num)), &start), true);
    }
    exit_code
}

/// The released lease with what its start spent before the subject ran, step by step, and the subject's own run, for a reader that times a check's sandbox start from its output.
fn started(released: Obj, start: &subject_start::Start) -> Obj {
    if start.steps.is_empty() {
        return released;
    }
    let steps: Vec<Value> = start.steps.iter().map(|(name, seconds)| serde_json::json!([name, round(*seconds, 3)])).collect();
    released.put("start", steps).put_some("subjectSeconds", start.subject_seconds.map(|seconds| round(seconds, 3)))
}
