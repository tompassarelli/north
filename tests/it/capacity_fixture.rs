use crate::capacity_host;

use capacity_host::{Exclusive, Host};
use serde_json::{Value, json};
use std::process::Output;

fn fixture(host: &Host, row: &str) -> Output {
    let words: Vec<&str> = row.split(' ').collect();
    let [profile, class, available, protected, full, leased_cpus, leased_mib, extra @ ..] = words.as_slice() else { panic!("fixture row {row}") };
    host.command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "host", "fixture", "--profile", profile, "--class", class, "--cores", "24", "--memory-total-mib", "96343", "--memory-available-mib", available])
        .args(["--protected-cpu-some-avg10-basis-points", protected, "--memory-full-avg10-basis-points", full, "--leased-cpus", leased_cpus, "--leased-memory-mib", leased_mib])
        .args(extra)
        .output_exclusive()
        .unwrap()
}

fn result(host: &Host, row: &str) -> Value {
    let output = fixture(host, row);
    let mut value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{row}: {}", String::from_utf8_lossy(&output.stderr)));
    value.as_object_mut().unwrap().remove("leasedCpuCeilings");
    value
}

fn decisions(rows: &[(&str, &str, &str)]) {
    let host = Host::new("capacity-fixture-", &[]);
    for (row, comparison, want) in rows {
        let got = fixture(&host, row);
        let decision = serde_json::from_slice::<Value>(&got.stdout).map_or(String::new(), |value| value["decision"].as_str().unwrap_or_default().to_owned());
        assert_eq!(decision == *want, *comparison == "==", "{row}: decided {decision:?}, want {comparison} {want}");
    }
}

#[test]
fn a_run_decision_states_the_scope_it_grants() {
    let host = Host::new("capacity-fixture-", &[]);
    assert_eq!(result(&host, "attended heavy 70000 500 0 6 8192"), json!({"decision": "RUN", "profile": "attended", "class": "heavy", "cpus": 6, "memoryMiB": 8192, "memoryFullAvg10": 0, "aggregateCpuLimit": 20}));
    assert_eq!(result(&host, "unattended exclusive 70000 500 0 0 0"), json!({"decision": "RUN", "profile": "unattended", "class": "exclusive", "cpus": 24, "memoryMiB": 16384, "memoryFullAvg10": 0, "aggregateCpuLimit": 24}));
    assert_eq!(result(&host, "attended native 70000 5000 0 0 0"), json!({"decision": "RUN", "profile": "attended", "class": "native", "cpus": 2, "memoryMiB": 4096, "memoryFullAvg10": 0, "aggregateCpuLimit": 20}));
}

#[test]
fn a_client_memory_request_sets_the_native_scope_and_malformed_or_batch_requests_are_refused() {
    let host = Host::new("capacity-fixture-", &[]);
    assert_eq!(result(&host, "unattended native 40000 0 0 0 70000 --memory-gib 1.5")["memoryMiB"], json!(1536));
    for memory in ["0", "-1", "NaN", "Infinity", "1.0001", "100000000000000"] {
        let refused = fixture(&host, &format!("unattended native 40000 0 0 0 0 --memory-gib {memory}"));
        assert!(!refused.status.success(), "invalid memory request accepted: {memory}");
    }
    assert!(!fixture(&host, "unattended heavy 40000 0 0 0 0 --memory-gib 1.5").status.success(), "batch memory override accepted");
}

#[test]
fn critical_and_tom_facing_work_lease_all_cores_less_the_attended_reserve() {
    let host = Host::new("capacity-fixture-", &[]);
    let critical = result(&host, "unattended critical 70000 0 0 0 0");
    assert_eq!((&critical["decision"], &critical["cpus"]), (&json!("RUN"), &json!(20)));
    let tom = result(&host, "attended tom 70000 0 0 0 0");
    assert_eq!((&tom["decision"], &tom["cpus"], &tom["memoryMiB"]), (&json!("RUN"), &json!(20), &json!(16384)));
}

#[test]
fn batch_yields_to_the_desktops_own_cpu_wait_only_when_present() {
    decisions(&[
        ("attended heavy 70000 1000 394 0 0", "==", "DEFER_INTERACTIVE_PRESSURE"),
        ("attended heavy 70000 999 394 0 0", "==", "RUN"),
        ("unattended heavy 70000 10000 394 0 0", "==", "RUN"),
    ]);
}

#[test]
fn declared_batch_cpus_past_the_aggregate_limit_do_not_queue_moderate_work() {
    decisions(&[
        ("attended moderate 70109 0 0 26 32768 --peer-batch-runs 7", "==", "RUN"),
        ("attended moderate 70109 0 0 18 32768 --peer-batch-runs 7", "==", "RUN"),
    ]);
}

#[test]
fn memory_floor_is_a_fifth_of_ram_present_and_eight_gib_away() {
    decisions(&[
        ("attended heavy 21000 0 394 0 0", "==", "DEFER_MEMORY_HEADROOM"),
        ("unattended heavy 21000 0 394 0 0", "==", "RUN"),
        ("unattended heavy 16000 0 394 0 0", "==", "DEFER_MEMORY_HEADROOM"),
        ("unattended moderate 10241 0 0 0 0", "==", "RUN"),
        ("unattended heavy 70000 0 394 0 70000", "==", "DEFER_MEMORY_CAPACITY"),
        ("attended agent 73412 5000 394 8 9728", "==", "RUN"),
        ("attended heavy 73412 0 10000 8 9728", "==", "RUN"),
    ]);
}

#[test]
fn native_clients_ignore_pressure_and_are_bounded_by_cores_and_memory() {
    decisions(&[
        ("attended native 70000 0 0 18 0 --leased-native-cpus 18", "==", "RUN"),
        ("attended native 70000 0 0 20 0 --leased-native-cpus 20", "==", "DEFER_NATIVE_CPUS"),
        ("unattended native 70000 0 0 20 0 --leased-native-cpus 20", "==", "RUN"),
        ("attended native 70000 0 0 24 16384 --peer-exclusive-runs 1", "==", "RUN"),
        ("attended native 12000 0 0 0 0", "==", "DEFER_MEMORY_HEADROOM"),
    ]);
}

#[test]
fn a_smaller_native_request_keeps_the_floor_and_the_leased_memory_cap() {
    decisions(&[
        ("unattended native 40000 0 0 0 70000", "==", "DEFER_MEMORY_CAPACITY"),
        ("unattended native 40000 0 0 0 70000 --memory-gib 1.5", "==", "RUN"),
        ("unattended native 9600 0 0 0 0 --memory-gib 1.5", "==", "DEFER_MEMORY_HEADROOM"),
        ("attended native 20000 0 0 0 0 --memory-gib 1.5", "==", "DEFER_MEMORY_HEADROOM"),
        ("unattended native 40000 0 0 0 71000 --memory-gib 1.5", "==", "DEFER_MEMORY_CAPACITY"),
    ]);
}

#[test]
fn headless_renders_share_the_gpu_two_at_a_time_and_yield_to_native_clients() {
    decisions(&[
        ("unattended gpu 80000 0 0 1 2048 --gpu-runs 1", "==", "RUN"),
        ("unattended gpu 80000 0 0 2 4096 --gpu-runs 2", "==", "DEFER_GPU_SLOTS"),
        ("unattended gpu 80000 0 0 0 0 --native-waiting 1", "==", "DEFER_NATIVE_WAITING"),
        ("unattended native 80000 0 0 0 0 --gpu-busy-percent 95", "==", "DEFER_GPU_BUSY"),
        ("unattended native 80000 0 0 2 4096 --gpu-busy-percent 95 --gpu-runs 2", "==", "RUN"),
        ("attended native 80000 0 0 0 0 --gpu-clients 4 --new-clients 1 --gpu-runs 1", "==", "DEFER_GPU_CLIENTS"),
    ]);
}

#[test]
fn leased_heavy_cpus_past_the_aggregate_limit_reserve_nothing_against_the_next_heavy_build() {
    decisions(&[
        ("attended heavy 80000 0 0 26 21504 --peer-batch-runs 3", "==", "RUN"),
        ("attended heavy 80000 0 0 14 21504 --peer-batch-runs 3", "==", "RUN"),
        ("unattended heavy 80000 0 0 18 21504 --peer-batch-runs 3", "==", "RUN"),
        ("unattended heavy 80000 0 0 44 21504 --peer-batch-runs 4", "==", "RUN"),
    ]);
}

#[test]
fn system_cpu_pressure_holds_moderate_work_at_thirty_percent_and_heavy_work_at_the_high_mark() {
    decisions(&[
        ("unattended heavy 80000 0 0 0 0 --cpu-some-avg10-basis-points 4000", "==", "DEFER_CPU_PRESSURE"),
        ("unattended heavy 80000 0 0 0 0 --cpu-some-avg10-basis-points 3999", "==", "RUN"),
        ("attended moderate 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400", "==", "DEFER_CPU_PRESSURE"),
        ("unattended exclusive 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400", "==", "RUN"),
        ("unattended native 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400", "==", "RUN"),
        ("unattended heavy 80000 0 0 0 0 --queued-ahead 1", "==", "DEFER_QUEUED"),
        ("unattended native 80000 0 0 0 0 --queued-ahead 1", "==", "RUN"),
        ("attended exclusive 80000 0 0 4 3072", "==", "RUN"),
        ("attended exclusive 80000 0 0 2 2048 --peer-batch-runs 1", "==", "DEFER_EXCLUSIVE"),
        ("unattended moderate 80000 0 0 18 16384 --peer-batch-runs 1 --peer-exclusive-runs 1", "==", "DEFER_EXCLUSIVE"),
        ("attended heavy 80000 0 0 6 8192 --peer-batch-runs 1 --unbounded-runs 1", "==", "DEFER_UNBOUNDED_PEER"),
    ]);
}

#[test]
fn a_waiting_exclusive_holds_later_heavy_work_and_waits_only_for_running_batch_leases() {
    decisions(&[
        ("unattended heavy 80000 0 0 0 0 --exclusive-waiting 1", "==", "DEFER_EXCLUSIVE_QUEUED"),
        ("attended moderate 80000 0 0 0 0 --exclusive-waiting 1", "==", "RUN"),
        ("unattended native 80000 0 0 0 0 --exclusive-waiting 1", "==", "RUN"),
        ("unattended exclusive 80000 0 0 0 0 --queued-ahead 3", "==", "RUN"),
        ("unattended exclusive 80000 0 0 0 0 --exclusive-waiting 1", "==", "DEFER_QUEUED"),
        ("attended exclusive 80000 5000 0 0 0 --cpu-some-avg10-basis-points 9000", "==", "RUN"),
        ("attended exclusive 80000 5000 0 6 8192 --peer-batch-runs 1", "==", "DEFER_EXCLUSIVE"),
    ]);
}

#[test]
fn critical_work_skips_the_queue_and_cpu_waits_but_keeps_memory_floors_and_a_running_exclusive() {
    decisions(&[
        ("attended critical 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --exclusive-waiting 1 --cpu-some-avg10-basis-points 9000", "==", "RUN"),
        ("unattended critical 20000 0 0 0 0", "==", "DEFER_MEMORY_HEADROOM"),
        ("unattended critical 80000 0 0 24 16384 --peer-batch-runs 1 --peer-exclusive-runs 1", "==", "DEFER_EXCLUSIVE"),
        ("attended critical 80000 1000 0 0 0", "==", "DEFER_INTERACTIVE_PRESSURE"),
    ]);
}

#[test]
fn tom_facing_builds_lease_criticals_scope_and_skip_the_queue() {
    decisions(&[
        ("attended tom 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --cpu-some-avg10-basis-points 9000", "==", "RUN"),
        ("attended moderate 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --cpu-some-avg10-basis-points 9000", "!=", "RUN"),
        ("attended tom 80000 1000 0 0 0", "==", "DEFER_INTERACTIVE_PRESSURE"),
        ("unattended heavy 80000 0 0 44 16384 --peer-batch-runs 1", "==", "RUN"),
        ("unattended exclusive 80000 0 0 20 16384 --peer-batch-runs 1", "==", "DEFER_EXCLUSIVE"),
    ]);
}

