mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Command, Stdio};

const AGENTS: &str = "#!/bin/sh\necho \"$XDG_STATE_HOME/machine-capacity/SKILL.md\"\n";

#[test]
fn sigterm_to_private_desktop_start_stops_its_desktop_and_releases_its_lease() {
    let host = Host::new(
        "desktop-signal-",
        &[("systemd-run", &scope_systemd_run("sleep 600")), ("systemctl", SCOPE_SYSTEMCTL), ("agents", AGENTS)],
    );
    fs::create_dir_all(host.path("state/machine-capacity/scripts")).unwrap();
    symlink(Host::bundle(), host.path("state/machine-capacity/scripts/machine-capacity.mjs")).unwrap();
    fs::write(host.path("render-node"), "").unwrap();
    let render = host.path("render-node");
    let mut start = host.command("bwrap");
    start
        .args(["--dev-bind", "/", "/", "--dev", "/dev", "--tmpfs", "/dev/dri", "--ro-bind"])
        .arg(&render)
        .args(["/dev/dri/renderD128", "--unshare-cgroup", "--", env!("CARGO_BIN_EXE_north"), "private-desktop", "start"])
        .args(["--render-node", "/dev/dri/renderD128"]);
    let mut wrapper = Wrapper(start.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap());
    host.until("the desktop lease and scope", |host| {
        host.leases("native") == 1
            && fs::read_dir(host.path("state/scopes")).unwrap().flatten().filter(|entry| fs::read_to_string(entry.path()).is_ok_and(|text| text.split_whitespace().last().is_some())).count() == 1
    });
    let lease = fs::read_dir(host.path("agent-capacity-v1/leases"))
        .unwrap()
        .flatten()
        .find(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .map(|entry| serde_json::from_str::<Value>(&fs::read_to_string(entry.path()).unwrap()).unwrap())
        .unwrap();
    let caller = lease["owner"].as_str().unwrap().strip_prefix("private-desktop:").unwrap().to_owned();
    let scope = fs::read_dir(host.path("state/scopes")).unwrap().flatten().next().unwrap().path();
    let desktop = fs::read_to_string(scope).unwrap().split_whitespace().last().unwrap().to_owned();

    assert!(Command::new("kill").args(["-TERM", &caller]).status().unwrap().success());
    host.until("the wrapper to exit", |_| wrapper.exited().is_some());
    host.until("the lease release", |host| host.leases("native") == 0);
    assert!(!fs::exists(format!("/proc/{desktop}")).unwrap(), "the desktop still runs");
    let stderr = wrapper.stderr();
    assert!(stderr.lines().any(|line| line.contains("\"RELEASED\"")), "{stderr}");
}
