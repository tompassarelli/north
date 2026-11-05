#![cfg(test)]

use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::agent_load::{self, Host, gaming_marked, process_start};

const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const NATIVE: &str = "0d5b08ff-b0f4-4db8-b7e2-abd6454e8814";
const HEAVY: &str = "faf4c30a-9c0c-4d72-8b7e-2abd6454e811";

fn sleeper() -> Child {
    Command::new("sleep").arg("300").spawn().unwrap()
}

fn host_pid(child: &Child) -> u32 {
    let local = child.id().to_string();
    let children = fs::read_to_string("/proc/thread-self/children").unwrap_or_default();
    let namespaced = |pid: &u32| {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        status.lines().find_map(|line| line.strip_prefix("NSpid:")).is_some_and(|ids| ids.split_whitespace().last() == Some(local.as_str()))
    };
    children.split_whitespace().filter_map(|pid| pid.parse::<u32>().ok()).find(namespaced).unwrap_or(child.id())
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

fn host(root: &Path, proc: &Path) -> Host {
    let log = root.join("systemctl.log");
    let cgroup = root.join("cgroup");
    let script = root.join("systemctl");
    crate::foundation::shim::write_script(
        &script,
        &format!(
            "echo \"$*\" >> '{}'\n[ -e '{}/fail-'\"$2\" ] && exit 1\n[ \"$2\" = stop ] && find '{}' -type d -name \"$3\" -exec cat {{}}/cgroup.procs \\; | xargs -r kill\nexit 0\n",
            log.display(),
            root.display(),
            cgroup.display()
        ),
    );
    Host {
        proc: proc.to_path_buf(),
        cgroup,
        capacity: root.join("capacity"),
        game_reserve: root.join("game-reserve.json"),
        systemctl: script.display().to_string(),
        uid: rustix::process::getuid().as_raw(),
    }
}

fn lease(host: &Host, id: &str, class: &str, slice: &str, wrapper: Option<(u32, String)>, members: &[u32]) {
    let leases = host.capacity.join("leases");
    fs::create_dir_all(&leases).unwrap();
    let (pid, start) = wrapper.map_or((None, None), |(pid, start)| (Some(pid), Some(start)));
    let record = serde_json::json!({ "id": id, "kind": "run", "class": class, "aggregateSlice": slice, "wrapperPid": pid, "wrapperStart": start });
    fs::write(leases.join(format!("{id}.json")), record.to_string()).unwrap();
    let path = slice.trim_end_matches(".slice").split('-').collect::<Vec<_>>();
    let mut scope = host.cgroup.clone();
    for index in 0..path.len() {
        scope.push(format!("{}.slice", path[..=index].join("-")));
    }
    scope.push(format!("agent-capacity-{}.scope", id.replace('-', "")));
    fs::create_dir_all(&scope).unwrap();
    let procs = members.iter().map(|pid| format!("{pid}\n")).collect::<String>();
    fs::write(scope.join("cgroup.procs"), procs).unwrap();
}

fn fixture(proc: &Path, pid: u32, comm: &str, exe: &str, cgroup: &str, env: &[&str]) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("stat"), format!("{pid} ({comm}) S 1 {pid} {pid} 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 4242 0 0\n")).unwrap();
    fs::write(dir.join("cgroup"), format!("0::{USER}/{cgroup}\n")).unwrap();
    fs::write(dir.join("cmdline"), format!("{exe}\0-launch\0")).unwrap();
    fs::write(dir.join("environ"), env.iter().map(|entry| format!("{entry}\0")).collect::<String>()).unwrap();
}

fn log(root: &Path) -> String {
    fs::read_to_string(root.join("systemctl.log")).unwrap_or_default()
}

fn frozen_state(host: &Host) -> (Option<u64>, Vec<String>) {
    let lease = fs::read(host.capacity.join("leases").join(format!("{HEAVY}.json"))).unwrap();
    let listed = fs::read(host.capacity.join("gaming-frozen.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    (serde_json::from_slice::<serde_json::Value>(&lease).unwrap()["frozenAt"].as_u64(), listed)
}

// KEEP(bug): an agent-owned Battle.net launcher is ignored while gaming still stops agent wine; regression of north#125 (launcher counted as gaming, commit 5e6cf225c)
#[test]
fn an_agent_owned_launcher_is_ignored_and_gaming_still_stops_agent_wine() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    let mut wine = sleeper();
    let scope = "native.slice/client.scope";
    fixture(&proc, wine.id(), "wineserver", "/proton/files/bin/wineserver", scope, &[]);
    fixture(&proc, 500, "Battle.net.exe", "C:\\Battle.net\\Battle.net.exe", scope, &[]);
    let processes = agent_load::processes(&host);
    agent_load::enforce(&host, true, true, &processes);
    assert!(exited(&mut wine), "an agent wine process survived gaming");
}

// KEEP(mutant): gaming stops agent clients and leaves Tom's game running; mutant gaming-kill-dropped (a34bb8ef8)
#[test]
fn gaming_stops_agent_clients_and_leaves_toms_game_running() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    let (mut tom, mut client, mut desktop, mut leaked) = (sleeper(), sleeper(), sleeper(), sleeper());
    let warcraft = "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe";
    fixture(&proc, tom.id(), "Warcraft III.ex", warcraft, "app.slice/app-steam-1.scope",
        &["STEAM_COMPAT_DATA_PATH=/home/tom/.local/share/Steam/steamapps/compatdata/3516115571"]);
    let native_scope = format!("native.slice/agent-capacity-{}.scope", NATIVE.replace('-', ""));
    fixture(&proc, client.id(), "Warcraft III.ex", warcraft, &native_scope,
        &["XDG_RUNTIME_DIR=/run/user/1000/private-desktop.mC5aee/runtime", "STEAM_COMPAT_DATA_PATH=/home/tom/.local/share/wisp/lan/clients/lan0a"]);
    let contained = "agent.slice/agent-capacity.slice/agent-contain-554824-1792304040.scope";
    fixture(&proc, desktop.id(), "labwc", "/nix/store/labwc/bin/labwc", contained, &["XDG_RUNTIME_DIR=/run/user/1000/private-desktop.OLgycV/runtime"]);
    fixture(&proc, leaked.id(), "Warcraft III.ex", warcraft, "app.slice/run-r1.service", &["WINEPREFIX=/home/tom/.local/share/wisp/online/clone-a/pfx"]);
    lease(&host, NATIVE, "native", "native.slice", None, &[client.id()]);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);

    let processes = agent_load::processes(&host);
    agent_load::declare_gaming(&host.capacity, true).unwrap();
    assert!(gaming_marked(&host.capacity));
    agent_load::enforce(&host, gaming_marked(&host.capacity), gaming_marked(&host.capacity), &processes);

    assert!(exited(&mut client) && exited(&mut desktop) && exited(&mut leaked), "an agent client or desktop survived gaming");
    assert!(tom.try_wait().unwrap().is_none(), "Tom's game was signalled");
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    assert!(log(root.path()).contains(&format!("--user freeze {heavy_unit}")));
    assert!(!log(root.path()).contains(&format!("stop {heavy_unit}")));

    agent_load::declare_gaming(&host.capacity, false).unwrap();
    agent_load::enforce(&host, gaming_marked(&host.capacity), gaming_marked(&host.capacity), &agent_load::processes(&host));
    assert!(log(root.path()).contains(&format!("--user thaw {heavy_unit}")));
    assert!(!gaming_marked(&host.capacity));
    assert!(tom.try_wait().unwrap().is_none(), "Tom's game was signalled after gaming ended");
    tom.kill().unwrap();
    tom.wait().unwrap();
}

// KEEP(mutant): a runner that dies leaves no client running; mutant orphan-reaper-dropped
#[test]
fn a_runner_that_dies_leaves_no_client_running() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), Path::new("/proc"));
    let (mut runner, mut live_runner) = (sleeper(), sleeper());
    let (mut orphan_client, mut kept_client) = (sleeper(), sleeper());
    let runner_host = host_pid(&runner);
    let live_runner_host = host_pid(&live_runner);
    let start = |pid: u32| process_start(&host, pid as i32).unwrap();
    lease(&host, NATIVE, "native", "native.slice", Some((runner_host, start(runner_host))), &[orphan_client.id()]);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", Some((live_runner_host, start(live_runner_host))), &[kept_client.id()]);
    runner.kill().unwrap();
    runner.wait().unwrap();

    agent_load::enforce(&host, false, false, &[]);

    assert!(exited(&mut orphan_client), "a client outlived its runner");
    assert!(kept_client.try_wait().unwrap().is_none(), "a client whose runner lives was stopped");
    for child in [&mut live_runner, &mut kept_client] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

// KEEP(bug): a refused thaw keeps the unit frozen and charged until a later thaw succeeds; regression of north#203 (cb80f3fbb)
#[test]
fn a_failed_thaw_keeps_the_unit_frozen_and_charged_until_a_later_thaw_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let thaws = || log(root.path()).matches(&format!("--user thaw {heavy_unit}")).count();

    agent_load::enforce(&host, true, true, &[]);
    let (frozen_at, listed) = frozen_state(&host);
    assert!(frozen_at.is_some());
    assert_eq!(listed, std::slice::from_ref(&heavy_unit));

    fs::write(root.path().join("fail-thaw"), "").unwrap();
    agent_load::enforce(&host, false, false, &[]);
    assert_eq!(thaws(), 1);
    assert_eq!(frozen_state(&host), (frozen_at, vec![heavy_unit.clone()]), "a refused thaw dropped the unit or its frozenAt");

    fs::remove_file(root.path().join("fail-thaw")).unwrap();
    agent_load::enforce(&host, false, false, &[]);
    assert_eq!(thaws(), 2, "the refused thaw was not retried");
    assert_eq!(frozen_state(&host), (None, Vec::new()), "the retried thaw left the unit listed or frozenAt set");
}

// KEEP(bug): a refused freeze leaves frozenAt unset and the next tick retries it; regression of north#203 (cb80f3fbb)
#[test]
fn a_failed_freeze_leaves_frozen_at_unset_and_the_next_tick_retries_it() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let freezes = || log(root.path()).matches(&format!("--user freeze {heavy_unit}")).count();

    fs::write(root.path().join("fail-freeze"), "").unwrap();
    agent_load::enforce(&host, true, true, &[]);
    assert_eq!(freezes(), 1);
    assert_eq!(frozen_state(&host), (None, Vec::new()), "a refused freeze was recorded as frozen");

    fs::remove_file(root.path().join("fail-freeze")).unwrap();
    agent_load::enforce(&host, true, true, &[]);
    assert_eq!(freezes(), 2, "the refused freeze was not retried");
    let (frozen_at, listed) = frozen_state(&host);
    assert!(frozen_at.is_some(), "the retried freeze left frozenAt unset");
    assert_eq!(listed, [heavy_unit]);
}

// KEEP(bug): a busy admission lock leaves frozenAt for the next tick to bring in line with the freeze; regression of north#203 (cb80f3fbb)
#[test]
fn a_busy_admission_lock_leaves_frozen_at_for_the_next_tick_to_bring_in_line_with_the_freeze() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let frozen_at = || {
        let lease = fs::read(host.capacity.join("leases").join(format!("{HEAVY}.json"))).unwrap();
        serde_json::from_slice::<serde_json::Value>(&lease).unwrap()["frozenAt"].as_u64()
    };
    let busy = || {
        let holder = rusqlite::Connection::open(host.capacity.join("admission.lock")).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
        holder
    };

    let holder = busy();
    agent_load::enforce(&host, true, true, &[]);
    drop(holder);
    assert!(log(root.path()).contains(&format!("--user freeze {heavy_unit}")));
    assert_eq!(frozen_at(), None, "the pause wrote through a busy lock");
    agent_load::enforce(&host, true, true, &[]);
    assert!(frozen_at().is_some(), "the unit stayed frozen without frozenAt after a tick with the lock free");

    let holder = busy();
    agent_load::enforce(&host, false, false, &[]);
    drop(holder);
    assert!(log(root.path()).contains(&format!("--user thaw {heavy_unit}")));
    assert!(frozen_at().is_some(), "the resume wrote through a busy lock");
    agent_load::enforce(&host, false, false, &[]);
    assert_eq!(frozen_at(), None, "the unit stayed thawed with frozenAt after a tick with the lock free");
}
