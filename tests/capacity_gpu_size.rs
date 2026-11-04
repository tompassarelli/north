mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::fs;

fn host() -> Host {
    let recording = scope_systemd_run("\"$@\"").replacen("#!/bin/sh\n", "#!/bin/sh\nprintf '%s\\n' \"$@\" >\"$XDG_STATE_HOME/systemd-run.args\"\n", 1);
    Host::new("capacity-gpu-size-", &[("systemd-run", &recording), ("systemctl", SCOPE_SYSTEMCTL)])
}

#[test]
fn a_gpu_run_sized_for_a_native_game_gets_its_cpu_quota_and_memory_high() {
    let host = host();
    let status = host.helper(&["run", "--class", "gpu", "--memory-gib", "4", "--cpus", "4", "--owner", "makgora-perf", "--timeout-seconds", "30", "--", "true"]).status().unwrap();
    assert!(status.success(), "{}", host.helper_stderr());
    assert!(host.helper_stderr().contains(r#""scope":{"cpus":4,"memoryMiB":4096}"#), "{}", host.helper_stderr());
    let arguments = fs::read_to_string(host.path("state/systemd-run.args")).unwrap();
    assert!(arguments.lines().any(|line| line == "--property=CPUQuota=400%"), "{arguments}");
    assert!(arguments.lines().any(|line| line == "--property=MemoryHigh=4096M"), "{arguments}");
}

#[test]
fn cpus_is_refused_outside_a_whole_gpu_request() {
    let host = host();
    for request in [&["--class", "native", "--cpus", "4"][..], &["--class", "heavy", "--cpus", "4"], &["--class", "gpu", "--cpus", "1.5"], &["--class", "gpu", "--cpus", "0"]] {
        let output = host.helper(&[&["probe"][..], request].concat()).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{request:?} was accepted");
    }
}
