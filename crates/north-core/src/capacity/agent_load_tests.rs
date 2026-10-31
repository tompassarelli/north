use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n[ -e '{}/fail-'\"$2\" ] && exit 1\n[ \"$2\" = stop ] && find '{}' -type d -name \"$3\" -exec cat {{}}/cgroup.procs \\; | xargs -r kill\nexit 0\n",
            log.display(),
            root.display(),
            cgroup.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    crate::foundation::shim::wait_until_executable(&script);
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
    agent_load::enforce(&host, gaming_marked(&host.capacity), false, &processes);

    assert!(exited(&mut client) && exited(&mut desktop) && exited(&mut leaked), "an agent client or desktop survived gaming");
    assert!(tom.try_wait().unwrap().is_none(), "Tom's game was signalled");
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    assert!(log(root.path()).contains(&format!("--user freeze {heavy_unit}")));
    assert!(!log(root.path()).contains(&format!("stop {heavy_unit}")));

    agent_load::declare_gaming(&host.capacity, false).unwrap();
    agent_load::enforce(&host, gaming_marked(&host.capacity), false, &agent_load::processes(&host));
    assert!(log(root.path()).contains(&format!("--user thaw {heavy_unit}")));
    assert!(!gaming_marked(&host.capacity));
    assert!(tom.try_wait().unwrap().is_none(), "Tom's game was signalled after gaming ended");
    tom.kill().unwrap();
    tom.wait().unwrap();
}

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

#[test]
fn the_sampler_reports_only_the_declared_hold_whatever_runs_or_idles() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    let sample = |idle| super::laptop_auto::sample(&host, idle, false, None);
    fixture(&proc, 123, "Warcraft III.ex", "C:\\Warcraft III.exe", "app.slice/app-steam-1.scope", &[]);
    fixture(&proc, 124, "proton", "/proton/files/bin/proton", "app.slice/app-steam-1.scope", &[]);
    let observed = sample(Some(false));
    assert!(observed["gaming"] == false && observed["idle"] == false && observed.get("gpu_busy").is_none() && observed.get("agent_gpu").is_none(), "{observed}");
    fs::create_dir_all(&host.capacity).unwrap();
    fs::write(host.capacity.join("gaming.json"), r#"{"gaming":true,"at":1}"#).unwrap();
    assert_eq!(sample(Some(true))["gaming"], true, "a declaration made long ago still holds");
    agent_load::declare_gaming(&host.capacity, false).unwrap();
    assert_eq!(sample(None)["gaming"], false);
}

#[test]
fn a_game_reserve_left_on_before_the_last_boot_no_longer_counts_as_gaming() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    fs::create_dir_all(&proc).unwrap();
    let host = host(root.path(), &proc);
    fs::write(proc.join("stat"), "cpu  1 2 3\nbtime 1792306000\nprocesses 9\n").unwrap();
    for (since, on) in [(1792305507, false), (1792306000, true), (1792309000, true)] {
        fs::write(&host.game_reserve, serde_json::json!({ "on": true, "since": since }).to_string()).unwrap();
        assert_eq!(agent_load::game_reserve_on(&host), on, "{since}");
    }
    fs::write(&host.game_reserve, r#"{"on": false}"#).unwrap();
    assert!(!agent_load::game_reserve_on(&host));
}

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
    agent_load::enforce(&host, true, false, &processes);
    assert!(exited(&mut wine), "an agent wine process survived gaming");
}

#[test]
fn laptop_observe_that_never_answers_ends_inside_its_wait_with_a_failure() {
    use std::process::Command;
    use std::time::{Duration, Instant};
    let wait = Duration::from_millis(300);
    let begun = Instant::now();
    assert_eq!(super::laptop_auto::observe(Command::new("sh").args(["-c", "exec sleep 30"]), wait), 1);
    assert!(begun.elapsed() < Duration::from_secs(3), "{:?}", begun.elapsed());
    assert_eq!(super::laptop_auto::observe(Command::new("sh").args(["-c", "exit 5"]), wait), 5);
    assert_eq!(super::laptop_auto::observe(&mut Command::new("true"), wait), 0);
}

#[test]
fn only_a_discharging_system_battery_reports_a_percent() {
    let root = tempfile::tempdir().unwrap();
    let supply = |name: &str, fields: &[(&str, &str)]| {
        fs::create_dir_all(root.path().join(name)).unwrap();
        for (field, value) in fields {
            fs::write(root.path().join(name).join(field), format!("{value}\n")).unwrap();
        }
    };
    supply("ACAD", &[("type", "Mains"), ("online", "0")]);
    supply("hid-trackpad-battery", &[("type", "Battery"), ("scope", "Device"), ("status", "Discharging"), ("capacity", "5")]);
    supply("BAT1", &[("type", "Battery"), ("status", "Charging"), ("capacity", "27")]);
    assert_eq!(super::laptop_auto::discharging_percent(root.path()), None);
    supply("BAT1", &[("status", "Discharging")]);
    assert_eq!(super::laptop_auto::discharging_percent(root.path()), Some(27));
    assert_eq!(super::laptop_auto::discharging_percent(&root.path().join("absent")), None);
}

#[test]
fn a_low_battery_warns_once_per_discharge() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("capacity/battery-warned");
    let log = root.path().join("notifications");
    let warn = |battery| {
        let mut notify = Command::new("sh");
        notify.args(["-c", "printf '%s\\n' \"$*\" >> \"$0\"", log.to_str().unwrap()]);
        super::laptop_auto::warn_battery_once(&marker, battery, &mut notify, Duration::from_secs(5));
        fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>()
    };
    assert!(warn(Some(31)).is_empty());
    assert_eq!(warn(Some(29)), ["--urgency=critical --app-name=laptop-auto Laptop battery at 29% and discharging Agent placement is draining off the laptop. Plug in AC."]);
    assert_eq!(warn(Some(20)).len(), 1);
    assert_eq!(warn(None).len(), 1);
    assert!(!marker.exists());
    assert_eq!(warn(Some(28)).len(), 2);
}

#[test]
fn a_battery_notification_that_hangs_or_fails_leaves_no_marker_and_ends_inside_its_wait() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("battery-warned");
    let begun = Instant::now();
    super::laptop_auto::warn_battery_once(&marker, Some(20), Command::new("sh").args(["-c", "exec sleep 30"]), Duration::from_millis(300));
    assert!(begun.elapsed() < Duration::from_secs(3), "{:?}", begun.elapsed());
    super::laptop_auto::warn_battery_once(&marker, Some(20), &mut Command::new("false"), Duration::from_secs(5));
    assert!(!marker.exists());
}

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
    agent_load::enforce(&host, true, false, &[]);
    drop(holder);
    assert!(log(root.path()).contains(&format!("--user freeze {heavy_unit}")));
    assert_eq!(frozen_at(), None, "the pause wrote through a busy lock");
    agent_load::enforce(&host, true, false, &[]);
    assert!(frozen_at().is_some(), "the unit stayed frozen without frozenAt after a tick with the lock free");

    let holder = busy();
    agent_load::enforce(&host, false, false, &[]);
    drop(holder);
    assert!(log(root.path()).contains(&format!("--user thaw {heavy_unit}")));
    assert!(frozen_at().is_some(), "the resume wrote through a busy lock");
    agent_load::enforce(&host, false, false, &[]);
    assert_eq!(frozen_at(), None, "the unit stayed thawed with frozenAt after a tick with the lock free");
}

fn frozen_state(host: &Host) -> (Option<u64>, Vec<String>) {
    let lease = fs::read(host.capacity.join("leases").join(format!("{HEAVY}.json"))).unwrap();
    let listed = fs::read(host.capacity.join("gaming-frozen.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    (serde_json::from_slice::<serde_json::Value>(&lease).unwrap()["frozenAt"].as_u64(), listed)
}

#[test]
fn a_failed_freeze_leaves_frozen_at_unset_and_the_next_tick_retries_it() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let freezes = || log(root.path()).matches(&format!("--user freeze {heavy_unit}")).count();

    fs::write(root.path().join("fail-freeze"), "").unwrap();
    agent_load::enforce(&host, true, false, &[]);
    assert_eq!(freezes(), 1);
    assert_eq!(frozen_state(&host), (None, Vec::new()), "a refused freeze was recorded as frozen");

    fs::remove_file(root.path().join("fail-freeze")).unwrap();
    agent_load::enforce(&host, true, false, &[]);
    assert_eq!(freezes(), 2, "the refused freeze was not retried");
    let (frozen_at, listed) = frozen_state(&host);
    assert!(frozen_at.is_some(), "the retried freeze left frozenAt unset");
    assert_eq!(listed, [heavy_unit]);
}

#[test]
fn a_failed_thaw_keeps_the_unit_frozen_and_charged_until_a_later_thaw_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy_unit = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let thaws = || log(root.path()).matches(&format!("--user thaw {heavy_unit}")).count();

    agent_load::enforce(&host, true, false, &[]);
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

#[test]
fn game_reserve_leaves_heavy_scopes_running_on_agent_cpus_and_still_freezes_gpu_and_exclusive() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let host = host(root.path(), &proc);
    let (gpu, exclusive) = ("6a1e0c2e-0000-4000-8000-000000000001", "6a1e0c2e-0000-4000-8000-000000000002");
    lease(&host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    lease(&host, gpu, "gpu", "agent-capacity.slice", None, &[]);
    lease(&host, exclusive, "exclusive", "agent-capacity.slice", None, &[]);
    let unit = |id: &str| format!("agent-capacity-{}.scope", id.replace('-', ""));

    agent_load::enforce(&host, true, false, &[]);
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(HEAVY))), "gaming outside a reserve left heavy running");

    agent_load::enforce(&host, true, true, &[]);
    assert!(log(root.path()).contains(&format!("--user thaw {}", unit(HEAVY))), "a heavy scope frozen before the reserve stayed frozen");
    let (heavy_frozen_at, mut listed) = frozen_state(&host);
    listed.sort();
    let mut held = vec![unit(gpu), unit(exclusive)];
    held.sort();
    assert_eq!((heavy_frozen_at, listed), (None, held));

    agent_load::enforce(&host, true, true, &[]);
    assert_eq!(log(root.path()).matches(&format!("--user freeze {}", unit(HEAVY))).count(), 1, "the reserve froze heavy again");
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(gpu))));
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(exclusive))));
}
