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
    agent_load::enforce(&host, true, true, &processes);
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
fn a_battery_alerts_once_on_charger_loss_and_once_per_upower_warning_level() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("capacity/battery-warned");
    let log = root.path().join("notifications");
    let alert = |battery, level| {
        let mut notify = Command::new("sh");
        notify.args(["-c", "printf '%s|' \"$@\" >> \"$0\"; echo >> \"$0\"", log.to_str().unwrap()]);
        super::laptop_auto::alert_battery(&marker, battery, level, &mut notify, Duration::from_secs(5));
        fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>()
    };
    let shutdown = "At the action level the laptop saves its memory to disk and the screen goes dark for about two minutes. Do not press or hold the power button. Plug in now.";
    let lost = "--urgency=critical|--app-name=laptop-auto|Laptop charger stopped: on battery at 80%|The charger or its cable dropped power while plugged in. Plug it in or reseat the cable.|";
    assert!(alert(None, None).is_empty());
    assert_eq!(alert(Some(80), Some(1)), [lost]);
    assert_eq!(alert(Some(79), Some(1)), [lost]);
    let low = "--urgency=critical|--app-name=laptop-auto|Laptop battery low at 20%|The laptop is still on battery. Plug in or reseat the charger.|";
    assert_eq!(alert(Some(20), Some(3)), [lost, low]);
    assert_eq!(alert(Some(19), Some(3)), [lost, low]);
    let critical = format!("--urgency=critical|--app-name=laptop-auto|Laptop battery critical at 10%|{shutdown}|");
    assert_eq!(alert(Some(10), Some(4)), [lost, low, &critical]);
    assert_eq!(alert(Some(10), Some(4)), [lost, low, &critical]);
    let action = format!("--urgency=critical|--app-name=laptop-auto|Laptop battery at 8%: saving memory to disk now|{shutdown}|");
    assert_eq!(alert(Some(8), Some(5)), [lost, low, &critical, &action]);
    assert_eq!(alert(Some(7), Some(5)).len(), 4);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "action\n");
    assert_eq!(alert(None, None).len(), 4);
    assert!(!marker.exists());
    let again = "--urgency=critical|--app-name=laptop-auto|Laptop charger stopped: on battery at 60%|The charger or its cable dropped power while plugged in. Plug it in or reseat the cable.|";
    assert_eq!(alert(Some(60), None).last().map(String::as_str), Some(again));
    assert_eq!(alert(Some(60), None).len(), 5);
}

#[test]
fn the_battery_warning_level_reads_from_upower_display_device() {
    let level = |script: &str| super::laptop_auto::warning_level(Command::new("sh").args(["-c", script, "busctl"]));
    assert_eq!(level("[ \"$*\" = 'get-property org.freedesktop.UPower /org/freedesktop/UPower/devices/DisplayDevice org.freedesktop.UPower.Device WarningLevel' ] && echo 'u 4'"), Some(4));
    assert_eq!(level("echo 'u 1'"), Some(1));
    assert_eq!(level("echo 'u 5'; exit 1"), None);
    assert_eq!(level("echo garbage"), None);
}

#[test]
fn a_battery_notification_that_hangs_or_fails_leaves_no_marker_and_ends_inside_its_wait() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("battery-warned");
    let begun = Instant::now();
    super::laptop_auto::alert_battery(&marker, Some(20), None, Command::new("sh").args(["-c", "exec sleep 30"]), Duration::from_millis(300));
    assert!(begun.elapsed() < Duration::from_secs(3), "{:?}", begun.elapsed());
    super::laptop_auto::alert_battery(&marker, Some(20), None, &mut Command::new("false"), Duration::from_secs(5));
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

    agent_load::enforce(&host, true, true, &[]);
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(HEAVY))), "gaming outside a reserve left heavy running");

    agent_load::enforce(&host, true, false, &[]);
    assert!(log(root.path()).contains(&format!("--user thaw {}", unit(HEAVY))), "a heavy scope frozen before the reserve stayed frozen");
    let (heavy_frozen_at, mut listed) = frozen_state(&host);
    listed.sort();
    let mut held = vec![unit(gpu), unit(exclusive)];
    held.sort();
    assert_eq!((heavy_frozen_at, listed), (None, held));

    agent_load::enforce(&host, true, false, &[]);
    assert_eq!(log(root.path()).matches(&format!("--user freeze {}", unit(HEAVY))).count(), 1, "the reserve froze heavy again");
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(gpu))));
    assert!(log(root.path()).contains(&format!("--user freeze {}", unit(exclusive))));
}

#[test]
fn protected_slice_pressure_freezes_the_batch_slice_within_one_tick_and_thaws_it_when_pressure_falls() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), Path::new("/proc"));
    let batch = host.cgroup.join("agent.slice/agent-batch.slice");
    fs::create_dir_all(&batch).unwrap();
    fs::write(batch.join("cgroup.freeze"), "0\n").unwrap();
    let pressure = |slice: &str, percent: f64| {
        fs::create_dir_all(host.cgroup.join(slice)).unwrap();
        fs::write(host.cgroup.join(slice).join("cpu.pressure"), format!("some avg10={percent:.2} avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n")).unwrap();
    };
    let frozen = || fs::read_to_string(batch.join("cgroup.freeze")).unwrap().trim() == "1";
    pressure("app.slice", 3.0);
    pressure("session.slice", 26.0);
    pressure("agent.slice/agent-batch.slice", 90.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, false), Some(agent_load::PressureHold { frozen: true, pressure_basis_points: 2600.0 }));
    assert!(frozen(), "the batch slice froze on the tick that read protected pressure past the attended high mark");
    pressure("session.slice", 12.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, false), None);
    assert!(frozen(), "the batch slice stays frozen inside the band");
    pressure("session.slice", 4.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, false).map(|hold| hold.frozen), Some(false));
    assert!(!frozen(), "the batch slice thawed once pressure fell under the low mark");
    pressure("game-reserve.slice", 21.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, false), None, "21% is under the attended high mark");
    assert_eq!(agent_load::hold_batch_under_pressure(&host, true).map(|hold| hold.frozen), Some(true), "the game band freezes at 20%");
    pressure("game-reserve.slice", 6.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, true), None, "6% is over the game low mark");
    pressure("game-reserve.slice", 4.0);
    assert_eq!(agent_load::hold_batch_under_pressure(&host, true).map(|hold| hold.frozen), Some(false));
    fs::remove_dir_all(&batch).unwrap();
    assert_eq!(agent_load::hold_batch_under_pressure(&host, false), None, "no batch slice, nothing to hold");
}


#[test]
fn a_pressure_frozen_batch_lease_keeps_its_deadline_and_a_capped_lease_does_not_pause() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), Path::new("/proc"));
    lease(&host, HEAVY, "heavy", "agent-batch.slice", None, &[]);
    lease(&host, NATIVE, "tom", "agent-capacity.slice", None, &[]);
    let record = |id: &str| host.capacity.join("leases").join(format!("{id}.json"));
    let read = |id: &str| serde_json::from_slice::<serde_json::Value>(&fs::read(record(id)).unwrap()).unwrap();
    for id in [HEAVY, NATIVE] {
        let mut lease = read(id);
        lease["expiresAt"] = 1_000_000u64.into();
        fs::write(record(id), lease.to_string()).unwrap();
    }
    let freeze = host.cgroup.join("agent.slice/agent-batch.slice/cgroup.freeze");
    fs::write(&freeze, "1\n").unwrap();
    agent_load::enforce(&host, false, false, &[]);
    assert!(read(HEAVY)["frozenAt"].is_u64(), "the frozen batch lease's deadline paused");
    assert!(read(NATIVE)["frozenAt"].is_null(), "a lease outside the batch slice keeps its clock");
    std::thread::sleep(Duration::from_millis(20));
    fs::write(&freeze, "0\n").unwrap();
    agent_load::enforce(&host, false, false, &[]);
    let thawed = read(HEAVY);
    assert!(thawed["frozenAt"].is_null());
    assert!(thawed["expiresAt"].as_u64().unwrap() > 1_000_000, "the deadline moved out by the time spent frozen");
    assert_eq!(read(NATIVE)["expiresAt"].as_u64(), Some(1_000_000));
}

#[test]
fn a_tick_on_battery_drains_laptop_placement_and_freezes_heavy_as_play_does_until_ac_returns() {
    use super::laptop_auto::{discharging_percent, on_battery, sample, shed};
    use super::mode::{Limits, Mode};
    use crate::ops::laptop::Ctx;
    let limits = Limits::bundled();
    let root = tempfile::tempdir().unwrap();
    let laptop = host(root.path(), &root.path().join("proc"));
    lease(&laptop, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    let heavy = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
    let battery = root.path().join("power_supply/BAT1");
    fs::create_dir_all(&battery).unwrap();
    for (field, value) in [("type", "Battery"), ("status", "Discharging"), ("capacity", "80")] {
        fs::write(battery.join(field), format!("{value}\n")).unwrap();
    }
    let placement = root.path().join("laptop");
    fs::create_dir_all(&placement).unwrap();
    fs::write(placement.join("window.json"), r#"{"tz":"UTC","open":"23:00","close":"10:00"}"#).unwrap();
    Ctx::fixture(&placement, "2026-11-05T00:00:00Z", "true").main(&["open".to_owned()]).unwrap();
    let tick = |now: &str| {
        let percent = discharging_percent(&root.path().join("power_supply"));
        let plan = shed(&laptop, on_battery(limits.effects(Mode::Desktop), percent), &[]);
        Ctx::fixture(&placement, now, "true").main(&["observe".to_owned(), sample(&laptop, None, false, percent).to_string()]).unwrap();
        let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(placement.join("placement/laptop.json")).unwrap()).unwrap();
        (plan, state)
    };

    let (drained, state) = tick("2026-11-05T00:01:00Z");
    assert_eq!((state["state"].as_str(), state["reason"].as_str(), state["by"].as_str()), (Some("draining"), Some("battery"), Some("laptop-auto")), "{state}");
    assert_eq!(drained.freeze, [heavy.clone()].into());
    assert_eq!(frozen_state(&laptop).1, vec![heavy.clone()]);
    let played = tempfile::tempdir().unwrap();
    let play_host = host(played.path(), &played.path().join("proc"));
    lease(&play_host, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
    assert_eq!(drained, shed(&play_host, limits.effects(Mode::Play).confined(false), &[]), "battery froze heavy batch as play mode does while its CPU split is not enforced");

    fs::write(battery.join("status"), "Charging\n").unwrap();
    let (restored, state) = tick("2026-11-05T00:02:00Z");
    assert_eq!((state["state"].as_str(), state["reason"].as_str()), (Some("available"), Some("window")), "{state}");
    assert_eq!(restored.thaw, [heavy.clone()].into());
    assert!(log(root.path()).contains(&format!("--user thaw {heavy}")));
    assert_eq!(frozen_state(&laptop), (None, Vec::new()));
}

struct PowerTicks {
    root: tempfile::TempDir,
    laptop: Host,
    heavy: String,
}

impl PowerTicks {
    fn new(line: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let laptop = host(root.path(), &root.path().join("proc"));
        lease(&laptop, HEAVY, "heavy", "agent-capacity.slice", None, &[]);
        let heavy = format!("agent-capacity-{}.scope", HEAVY.replace('-', ""));
        let placement = root.path().join("laptop");
        fs::create_dir_all(&placement).unwrap();
        fs::write(placement.join("window.json"), r#"{"tz":"UTC","open":"23:00","close":"10:00"}"#).unwrap();
        crate::ops::laptop::Ctx::fixture(&placement, "2026-11-05T00:00:00Z", "true").main(&["open".to_owned()]).unwrap();
        let ticks = Self { root, laptop, heavy };
        ticks.supply("hid-mouse", &[("type", "USB"), ("scope", "Device"), ("online", "1")]);
        for (name, kind) in line {
            ticks.supply(name, &[("type", kind), ("online", "1")]);
        }
        ticks.supply("BAT1", &[("type", "Battery"), ("status", "Charging"), ("capacity", "60")]);
        ticks
    }

    fn supply(&self, name: &str, fields: &[(&str, &str)]) {
        let dir = self.root.path().join("power_supply").join(name);
        fs::create_dir_all(&dir).unwrap();
        for (field, value) in fields {
            fs::write(dir.join(field), format!("{value}\n")).unwrap();
        }
    }

    fn tick(&self, now: u64, status: &str) -> (Option<u8>, super::agent_load::Plan, Vec<String>, Option<String>) {
        use super::laptop_auto::{alert_battery, battery, discharging_since, on_battery, sample, shed};
        use super::mode::{Limits, Mode};
        self.supply("BAT1", &[("status", status)]);
        let percent = battery(&self.root.path().join("power_supply"), &discharging_since(&self.laptop), now);
        let plan = shed(&self.laptop, on_battery(Limits::bundled().effects(Mode::Desktop), percent), &[]);
        let placement = self.root.path().join("laptop");
        let at = crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(1_793_836_860 + now).unwrap()).iso();
        let observed = sample(&self.laptop, None, false, percent).to_string();
        crate::ops::laptop::Ctx::fixture(&placement, &at, "true").main(&["observe".to_owned(), observed]).unwrap();
        let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(placement.join("placement/laptop.json")).unwrap()).unwrap();
        let log = self.root.path().join("notifications");
        let mut notify = Command::new("sh");
        notify.args(["-c", "printf '%s|' \"$@\" >> \"$0\"; echo >> \"$0\"", log.to_str().unwrap()]);
        alert_battery(&self.laptop.capacity.join("battery-warned"), percent, None, &mut notify, Duration::from_secs(5));
        let alerts = fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect();
        (percent, plan, alerts, state["reason"].as_str().map(str::to_owned))
    }

    fn frozen(&self) -> Vec<String> {
        frozen_state(&self.laptop).1
    }
}

const CHARGER_LOST: &str = "--urgency=critical|--app-name=laptop-auto|Laptop charger stopped: on battery at 60%|The charger or its cable dropped power while plugged in. Plug it in or reseat the cable.|";

#[test]
fn a_discharging_blip_with_the_charger_online_raises_no_alert_hold_or_freeze() {
    for kind in ["Mains", "USB"] {
        let ticks = PowerTicks::new(&[("ADP1", kind)]);
        let (percent, plan, alerts, reason) = ticks.tick(0, "Discharging");
        assert_eq!((percent, plan, alerts, reason.as_deref()), (None, Default::default(), Vec::new(), Some("open")), "{kind}");
        assert!(ticks.frozen().is_empty(), "{kind}");
        let (percent, plan, alerts, reason) = ticks.tick(30, "Charging");
        assert_eq!((percent, plan, alerts, reason.as_deref()), (None, Default::default(), Vec::new(), Some("open")), "{kind}");
        let (percent, _, alerts, _) = ticks.tick(150, "Discharging");
        assert_eq!((percent, alerts.len()), (None, 0), "a flicker after a charging tick starts the 120 s over for {kind}");
        assert!(ticks.frozen().is_empty(), "{kind}");
    }
}

#[test]
fn every_line_supply_offline_puts_the_laptop_on_battery_on_that_same_tick_and_ac_back_releases_it_next_tick() {
    let ticks = PowerTicks::new(&[("ACAD", "Mains"), ("ucsi-source-psy-USBC000:001", "USB")]);
    ticks.tick(0, "Charging");
    ticks.supply("ACAD", &[("online", "0")]);
    let (percent, _, alerts, _) = ticks.tick(20, "Discharging");
    assert_eq!((percent, alerts.len()), (None, 0), "one line supply still online holds the decision back");
    ticks.supply("ucsi-source-psy-USBC000:001", &[("online", "0")]);
    let (percent, plan, alerts, reason) = ticks.tick(40, "Discharging");
    assert_eq!((percent, alerts, reason.as_deref()), (Some(60), vec![CHARGER_LOST.to_owned()], Some("battery")));
    assert_eq!(plan.freeze, [ticks.heavy.clone()].into());
    assert_eq!(ticks.frozen(), vec![ticks.heavy.clone()]);

    ticks.supply("ACAD", &[("online", "1")]);
    let (percent, plan, alerts, reason) = ticks.tick(60, "Charging");
    assert_eq!((percent, alerts.len(), reason.as_deref()), (None, 1, Some("window")));
    assert_eq!(plan.thaw, [ticks.heavy.clone()].into());
    assert!(ticks.frozen().is_empty());
    assert!(!ticks.laptop.capacity.join("battery-warned").exists());
}

#[test]
fn discharging_with_the_charger_online_for_120_seconds_of_consecutive_ticks_counts_as_on_battery_until_ac_returns() {
    let ticks = PowerTicks::new(&[("ACAD", "Mains")]);
    for now in [1_000, 1_030, 1_060, 1_090, 1_119] {
        let (percent, plan, alerts, reason) = ticks.tick(now, "Discharging");
        assert_eq!((percent, plan, alerts, reason.as_deref()), (None, Default::default(), Vec::new(), Some("open")), "at {now}");
    }
    assert!(ticks.frozen().is_empty());
    let (percent, plan, alerts, reason) = ticks.tick(1_120, "Discharging");
    assert_eq!((percent, alerts, reason.as_deref()), (Some(60), vec![CHARGER_LOST.to_owned()], Some("battery")));
    assert_eq!(plan.freeze, [ticks.heavy.clone()].into());
    let (percent, _, alerts, _) = ticks.tick(1_150, "Discharging");
    assert_eq!((percent, alerts.len()), (Some(60), 1));

    let (percent, plan, alerts, reason) = ticks.tick(1_180, "Charging");
    assert_eq!((percent, alerts.len(), reason.as_deref()), (None, 1, Some("window")));
    assert_eq!(plan.thaw, [ticks.heavy.clone()].into());
    assert!(ticks.frozen().is_empty());
    assert!(!ticks.laptop.capacity.join("battery-discharging-since").exists());
    let (percent, _, alerts, _) = ticks.tick(1_210, "Discharging");
    assert_eq!((percent, alerts.len()), (None, 1));
}
