mod capacity_host;

use capacity_host::Host;
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn pair_admission(nvidia_smi: &str) -> (Value, Duration) {
    let host = Host::new("capacity-aperture-", &[("nvidia-smi", nvidia_smi)]);
    let facts = json!({
        "profile": "unattended", "class": "native", "cores": 24, "memoryTotalMiB": 96343.0, "memoryAvailableMiB": 80000.0,
        "cpuSomeAvg10BasisPoints": 0, "protectedCpuSomeAvg10BasisPoints": 0, "leasedMemoryMiB": 0.0, "leasedBatchCpus": 0.0,
        "leasedNativeCpus": 0.0, "requestedCpus": 2.0, "requestedMemoryMiB": 4096.0, "peerBatchRuns": 0, "peerExclusiveRuns": 0,
        "unboundedRuns": 0, "queuedAhead": 0, "exclusiveWaiting": 0, "gpuBusyPercent": 0, "gpuClients": 0,
        "newClients": 2, "newDesktops": 2, "gpuRuns": 0, "nativeWaiting": 0, "quietNativeRuns": 0, "besideNative": false,
    });
    let started = Instant::now();
    let mut child = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "admission"])
        .env_remove("AGENT_CAPACITY_NVIDIA_MEMORY")
        .env("AGENTS_ORCHESTRATION", host.path("orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", host.path(""))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(facts.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    (serde_json::from_slice(&output.stdout).unwrap(), elapsed)
}

#[test]
fn an_nvidia_smi_on_path_that_hangs_defers_a_new_desktop_pair_within_the_bound() {
    let (admission, elapsed) = pair_admission("#!/bin/sh\nexec sleep 30\n");
    assert_eq!(admission, json!({ "reason": "DEFER_GPU_APERTURE_TIMEOUT" }));
    assert!(elapsed < Duration::from_millis(2000), "{elapsed:?}");
}

#[test]
fn an_nvidia_smi_on_path_reporting_a_full_bar1_defers_a_new_desktop_pair() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/skills/machine-capacity/scripts/nvidia-memory-bar1-17-free.txt");
    let (admission, _) = pair_admission(&format!("#!/bin/sh\nexec cat '{}'\n", fixture.display()));
    assert_eq!((&admission["reason"], &admission["bar1FreeMiB"]), (&json!("DEFER_GPU_APERTURE"), &json!(17.0)));
    assert!(admission["bar1NeedMiB"].as_f64().is_some_and(|need| need > 17.0), "{admission}");
}
