use north_core::foundation::shim::write_script;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;

const SESSION: &str = "n-626-repro";
const FAR_SSH: &str = "shift $(($# - 1))\nexec sh -c \"$1\"\n";
const FAR_SYSTEMD_RUN: &str = "for argument; do case $argument in --unit=*) echo \"${argument#--unit=}\" >\"$SCOPE_RECORD\" ;; esac; done\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";

// KEEP(bug): on 2026-11-06 a laptop session's ssh work on gex45 showed as "no fleet session (ssh)" and the watchdog capped a bystander; work started through the session's ssh path names the session
#[test]
fn a_remote_command_through_the_fleet_ssh_path_runs_in_a_scope_the_far_watchdog_names_for_the_session() {
    let scratch = tempfile::tempdir().unwrap();
    let (shim, far) = (scratch.path().join("shim"), scratch.path().join("far"));
    fs::create_dir_all(&shim).unwrap();
    fs::create_dir_all(&far).unwrap();
    symlink(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/ssh"), shim.join("ssh")).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), far.join("north")).unwrap();
    write_script(&far.join("ssh"), FAR_SSH);
    write_script(&far.join("systemd-run"), FAR_SYSTEMD_RUN);
    let _bus = UnixListener::bind(scratch.path().join("bus")).unwrap();
    let record = scratch.path().join("scope");
    let path = format!("{}:{}:{}", shim.display(), far.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "gex45", "echo", "far 'work'", "\"$((6 * 7))\""])
        .env("PATH", &path)
        .env("NORTH_FLEET_SESSION", SESSION)
        .env("XDG_RUNTIME_DIR", scratch.path())
        .env("SHELL", "/bin/sh")
        .env("SCOPE_RECORD", &record)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "far work 42");
    let unit = fs::read_to_string(&record).unwrap().trim().to_owned();
    assert!(unit.starts_with(&format!("north-ssh-{SESSION}-")) && unit.ends_with(".scope"), "{unit}");
    let samples: Vec<String> = (0..3)
        .map(|at| {
            serde_json::json!({
                "at": format!("2026-11-06T02:45:{at:02}Z"), "load": [42.26, 40.0, 30.0], "cpuPsi": 80.0, "cpuPsi60": 60.0, "cpus": 20.0, "busyCores": 18.08,
                "cgroups": [{ "cgroup": format!("app.slice/{unit}"), "cores": 17.0 }, { "cgroup": "agent.slice/north-fleet-wisp-230-d1945-0f1e2d3c.service", "cores": 1.08 }],
            })
            .to_string()
        })
        .collect();
    let recorded = scratch.path().join("samples.jsonl");
    fs::write(&recorded, samples.join("\n")).unwrap();
    let replay = Command::new(env!("CARGO_BIN_EXE_north")).args(["capacity", "watchdog", "replay"]).arg(&recorded).args(["--cpus", "20"]).output().unwrap();
    let actions = String::from_utf8_lossy(&replay.stdout);
    assert!(replay.status.success(), "{}", String::from_utf8_lossy(&replay.stderr));
    assert!(actions.contains(&format!("session {SESSION}: 17 cores in {unit}")) && !actions.contains("no fleet session"), "{actions}");
}
