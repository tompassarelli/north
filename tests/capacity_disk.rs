mod capacity_host;

use capacity_host::Host;
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;

fn heavy_admission(disk_free_gib: f64) -> Value {
    let host = Host::new("capacity-disk-", &[]);
    let facts = json!({
        "profile": "unattended", "class": "heavy", "cores": 24, "memoryTotalMiB": 96343.0, "memoryAvailableMiB": 80000.0,
        "cpuSomeAvg10BasisPoints": 0, "protectedCpuSomeAvg10BasisPoints": 0, "leasedMemoryMiB": 0.0, "leasedBatchCpus": 0.0,
        "leasedNativeCpus": 0.0, "requestedCpus": 6.0, "requestedMemoryMiB": 8192.0, "peerBatchRuns": 0, "peerExclusiveRuns": 0,
        "unboundedRuns": 0, "queuedAhead": 0, "exclusiveWaiting": 0, "gpuBusyPercent": 0, "gpuClients": 0,
        "newClients": 0, "newDesktops": 0, "gpuRuns": 0, "nativeWaiting": 0, "besideNative": false, "diskFreeGiB": disk_free_gib,
    });
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command
        .args(["capacity", "admission"])
        .env("AGENTS_ORCHESTRATION", host.path("orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", host.path(""))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(facts.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_heavy_build_is_deferred_naming_the_free_space_when_the_root_disk_is_under_the_floor() {
    let admission = heavy_admission(9.5);
    assert_eq!((&admission["reason"], &admission["diskFreeGiB"]), (&json!("DEFER_DISK_FREE"), &json!(9.5)));
    assert_eq!(heavy_admission(40.0), json!({ "reason": "RUN" }));
}
