use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::capacity::agent_load::Host;

const HEAVY: &str = "faf4c30a-9c0c-4d72-8b7e-2abd6454e811";

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

fn log(root: &Path) -> String {
    fs::read_to_string(root.join("systemctl.log")).unwrap_or_default()
}

fn frozen_state(host: &Host) -> (Option<u64>, Vec<String>) {
    let lease = fs::read(host.capacity.join("leases").join(format!("{HEAVY}.json"))).unwrap();
    let listed = fs::read(host.capacity.join("gaming-frozen.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    (serde_json::from_slice::<serde_json::Value>(&lease).unwrap()["frozenAt"].as_u64(), listed)
}

#[test]
fn a_tick_on_battery_drains_laptop_placement_and_freezes_heavy_as_play_does_until_ac_returns() {
    use crate::capacity::laptop_auto::{discharging_percent, on_battery, sample, shed};
    use crate::capacity::mode::{Limits, Mode};
    use super::Ctx;
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
        super::Ctx::fixture(&placement, "2026-11-05T00:00:00Z", "true").main(&["open".to_owned()]).unwrap();
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

    fn tick(&self, now: u64, status: &str) -> (Option<u8>, crate::capacity::agent_load::Plan, Vec<String>, Option<String>) {
        use crate::capacity::laptop_auto::{alert_battery, battery, discharging_since, on_battery, sample, shed};
        use crate::capacity::mode::{Limits, Mode};
        self.supply("BAT1", &[("status", status)]);
        let percent = battery(&self.root.path().join("power_supply"), &discharging_since(&self.laptop), now);
        let plan = shed(&self.laptop, on_battery(Limits::bundled().effects(Mode::Desktop), percent), &[]);
        let placement = self.root.path().join("laptop");
        let at = crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(1_793_836_860 + now).unwrap()).iso();
        let observed = sample(&self.laptop, None, false, percent).to_string();
        super::Ctx::fixture(&placement, &at, "true").main(&["observe".to_owned(), observed]).unwrap();
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
