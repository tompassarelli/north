mod capacity_host;

use capacity_host::Host;
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const ORPHAN: &str = "0d5b08ff-b0f4-4db8-b7e2-abd6454e8814";
const LIVE: &str = "faf4c30a-9c0c-4d72-8b7e-2abd6454e811";
const LAPTOP_OBSERVE: &str = "#!/bin/sh
echo observed >> \"$XDG_STATE_HOME/laptop-called\"
";
const STOP_BY_KILLING_MEMBERS: &str = "#!/bin/sh
echo \"$*\" >> \"$XDG_STATE_HOME/systemctl.log\"
[ \"$2\" = stop ] && find \"$AGENT_LOAD_CGROUP\" -type d -name \"$3\" -exec cat {}/cgroup.procs \\; | xargs -r kill
exit 0
";

fn sleeper() -> Child {
    Command::new("sleep").arg("300").spawn().unwrap()
}

fn start(pid: u32) -> String {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat.rsplit_once(") ").unwrap().1.split(' ').nth(19).unwrap().to_owned()
}

fn lease(host: &Host, id: &str, class: &str, slice: &str, wrapper: u32, member: u32) {
    fs::create_dir_all(host.path("agent-capacity-v1/leases")).unwrap();
    let record = json!({ "id": id, "kind": "run", "class": class, "aggregateSlice": slice, "wrapperPid": wrapper, "wrapperStart": start(wrapper) });
    fs::write(host.path(&format!("agent-capacity-v1/leases/{id}.json")), record.to_string()).unwrap();
    let scope = host.path(&format!("user-cgroup/{slice}/agent-capacity-{}.scope", id.replace('-', "")));
    fs::create_dir_all(&scope).unwrap();
    fs::write(scope.join("cgroup.procs"), format!("{member}\n")).unwrap();
}

fn exited(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn the_orphan_reaper_stops_a_dead_wrappers_scope_on_a_host_with_no_laptop_command_and_spares_a_live_one() {
    let host = Host::new("capacity-orphan-reaper-", &[("systemctl", STOP_BY_KILLING_MEMBERS), ("laptop", LAPTOP_OBSERVE)]);
    fs::create_dir_all(host.path("proc")).unwrap();
    fs::write(host.path("proc/stat"), "btime 1\n").unwrap();
    let (mut dead_wrapper, mut live_wrapper, mut orphan_client, mut kept_client) = (sleeper(), sleeper(), sleeper(), sleeper());
    for wrapper in [&dead_wrapper, &live_wrapper] {
        symlink(format!("/proc/{}", wrapper.id()), host.path(&format!("proc/{}", wrapper.id()))).unwrap();
    }
    lease(&host, ORPHAN, "native", "native.slice", dead_wrapper.id(), orphan_client.id());
    lease(&host, LIVE, "native", "native.slice", live_wrapper.id(), kept_client.id());
    dead_wrapper.kill().unwrap();
    dead_wrapper.wait().unwrap();

    let output = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "reap-orphans"])
        .env("AGENT_LOAD_PROC", host.path("proc"))
        .env("AGENT_LOAD_CGROUP", host.path("user-cgroup"))
        .env("AGENT_LOAD_SYSTEMCTL", host.path("bin/systemctl"))
        .env("LAPTOP_AUTO_LAPTOP", host.path("bin/laptop"))
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(exited(&mut orphan_client), "a client outlived its dead wrapper");
    assert!(!host.path("state/laptop-called").exists(), "the reaper observed the laptop");
    assert!(kept_client.try_wait().unwrap().is_none(), "a client whose wrapper lives was stopped");
    let stopped = format!("agent-capacity-{}.scope", ORPHAN.replace('-', ""));
    assert!(String::from_utf8_lossy(&output.stdout).contains(&stopped), "{}", String::from_utf8_lossy(&output.stdout));
    for child in [&mut live_wrapper, &mut kept_client] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
