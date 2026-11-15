#![cfg(test)]

use std::fs;
use std::path::Path;

use super::game_reserve::Settings;

const UPTIME: f64 = 1000.0;
const SLICES: [&str; 6] = ["agent.slice", "background.slice", "control.slice", "native.slice", "user.slice", "builds.slice"];
const TOM_SLICES: [&str; 3] = ["app.slice", "session.slice", "game.slice"];
const NESTED: &str = "agent.slice/agent-capacity.slice";

pub(super) fn settings(root: &Path) -> Settings {
    let proc = root.join("proc");
    fs::create_dir_all(&proc).unwrap();
    fs::write(proc.join("uptime"), format!("{UPTIME:.2} 5000.00\n")).unwrap();
    let log = root.join("calls.log");
    let cgroup = root.join("cgroup");
    for slice in SLICES.iter().chain(&TOM_SLICES) {
        fs::create_dir_all(cgroup.join(slice)).unwrap();
        fs::write(cgroup.join(slice).join("cpuset.cpus.effective"), "0-23\n").unwrap();
    }
    fs::create_dir_all(cgroup.join("init.scope")).unwrap();
    fs::create_dir_all(cgroup.join(NESTED)).unwrap();
    fs::write(cgroup.join(NESTED).join("cpuset.cpus.effective"), "0-23\n").unwrap();
    let kernel = format!(
        "[ \"$2\" = set-property ] || exit 0\nslice=$4; shift 4\nfor p; do case $p in AllowedCPUs=*) printf '%s\\n' \"${{p#AllowedCPUs=}}\" > \"$(find '{}' -type d -name \"$slice\")\"/cpuset.cpus.effective 2>/dev/null;; esac; done\n",
        cgroup.display()
    );
    for tool in ["systemctl", "busctl"] {
        let script = root.join(tool);
        let body = if tool == "systemctl" { kernel.as_str() } else { "" };
        crate::foundation::shim::write_script(&script, &format!("echo \"{tool} $*\" >> '{}'\n{body}", log.display()));
    }
    let _ = fs::remove_file(&log);
    Settings {
        proc,
        cgroup,
        state: root.join("state.json"),
        systemctl: root.join("systemctl").display().to_string(),
        busctl: root.join("busctl").display().to_string(),
        affinity_log: Some(root.join("pins.log")),
        game_cpus: "0-3,12-15".into(),
        agent_cpus: "4-11,16-23".into(),
        agent_memory_max: "80G".into(),
    }
}
