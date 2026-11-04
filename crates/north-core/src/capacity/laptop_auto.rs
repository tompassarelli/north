use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use super::agent_load::{self, Host, Plan, Process};
use super::mode::Effects;
use crate::platform::process::status_within;

const USAGE: &str = "usage: north capacity laptop-auto [--dry-run]";
const OBSERVE_WAIT: Duration = Duration::from_secs(40);
const NOTIFY_WAIT: Duration = Duration::from_secs(5);
const BATTERY_ALERTED: &str = "battery-warned";
const DISCHARGING_SINCE: &str = "battery-discharging-since";
const UNDERPOWERED_CHARGER: Duration = Duration::from_secs(120);

pub fn sample(host: &Host, idle: Option<bool>, sleeping: bool, battery: Option<u8>) -> Value {
    json!({
        "gaming": agent_load::gaming_marked(&host.capacity),
        "idle": idle,
        "sleeping": sleeping,
        "battery": battery,
    })
}

pub fn power_supply() -> PathBuf {
    std::env::var_os("AGENT_LOAD_POWER_SUPPLY").filter(|value| !value.is_empty()).map_or_else(|| "/sys/class/power_supply".into(), PathBuf::from)
}

fn read(supply: &Path, field: &str) -> Option<String> {
    fs::read_to_string(supply.join(field)).ok().map(|value| value.trim().to_owned())
}

fn system_supplies<'a>(power_supply: &Path, types: &'a [&str]) -> impl Iterator<Item = PathBuf> + 'a {
    fs::read_dir(power_supply)
        .into_iter()
        .flatten()
        .flatten()
        .map(|supply| supply.path())
        .filter(|supply| read(supply, "type").is_some_and(|kind| types.contains(&kind.as_str())) && read(supply, "scope").as_deref() != Some("Device"))
}

pub fn discharging_percent(power_supply: &Path) -> Option<u8> {
    system_supplies(power_supply, &["Battery"])
        .filter(|supply| read(supply, "status").as_deref() == Some("Discharging"))
        .filter_map(|supply| read(&supply, "capacity")?.parse::<u8>().ok())
        .filter(|percent| *percent <= 100)
        .min()
}

fn line_power_online(power_supply: &Path) -> bool {
    system_supplies(power_supply, &["Mains", "USB"]).any(|supply| read(&supply, "online").as_deref() == Some("1"))
}

pub fn battery(power_supply: &Path, since: &Path, now: u64) -> Option<u8> {
    if discharging_percent(power_supply).is_none() {
        let _ = fs::remove_file(since);
        return None;
    }
    if line_power_online(power_supply)
        && recorded_since(since).is_none()
        && let Err(error) = since.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(since, format!("{now}\n")))
    {
        eprintln!("north capacity laptop-auto: {}: {error}", since.display());
    }
    settled_battery(power_supply, since, now)
}

pub fn settled_battery(power_supply: &Path, since: &Path, now: u64) -> Option<u8> {
    let percent = discharging_percent(power_supply)?;
    if !line_power_online(power_supply) {
        return Some(percent);
    }
    let first = recorded_since(since)?;
    (now.saturating_sub(first) >= UNDERPOWERED_CHARGER.as_secs()).then_some(percent)
}

fn recorded_since(since: &Path) -> Option<u64> {
    fs::read_to_string(since).ok().and_then(|text| text.trim().parse::<u64>().ok())
}

pub fn discharging_since(host: &Host) -> PathBuf {
    host.capacity.join(DISCHARGING_SINCE)
}

fn now() -> u64 {
    std::env::var("LAPTOP_AUTO_NOW").ok().and_then(|text| text.parse().ok()).unwrap_or_else(super::mode::now)
}

pub fn on_battery(effects: Effects, battery: Option<u8>) -> Effects {
    Effects { pause_heavy: effects.pause_heavy || battery.is_some(), ..effects }
}

pub fn shed(host: &Host, effects: Effects, processes: &[Process]) -> Plan {
    let gaming = effects.hold || effects.reserve;
    let game_reserve = effects.reserve && !effects.pause_heavy;
    if let Some(hold) = agent_load::hold_batch_under_pressure(host, game_reserve) {
        println!("agent-load: batch slice {} at protected CPU pressure {:.2}%", if hold.frozen { "frozen" } else { "thawed" }, hold.pressure_basis_points / 100.0);
    }
    let plan = agent_load::enforce(host, gaming, effects.pause_heavy, processes);
    if plan != Plan::default() {
        println!(
            "agent-load: gaming={gaming} pause_heavy={} stopped {:?} froze {:?} thawed {:?} killed {:?}",
            effects.pause_heavy, plan.stop, plan.freeze, plan.thaw, plan.kill
        );
    }
    plan
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BatteryAlert {
    ChargerLost,
    Low,
    Critical,
    Action,
}

impl BatteryAlert {
    const SHUTDOWN_WARNING: &str = "At the action level the laptop saves its memory to disk and the screen goes dark for about two minutes. Do not press or hold the power button. Plug in now.";

    fn reached(warning_level: Option<u8>) -> Self {
        match warning_level {
            Some(3) => Self::Low,
            Some(4) => Self::Critical,
            Some(5) => Self::Action,
            _ => Self::ChargerLost,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ChargerLost => "charger-lost",
            Self::Low => "low",
            Self::Critical => "critical",
            Self::Action => "action",
        }
    }

    fn recorded(marker: &Path) -> Option<Self> {
        let name = fs::read_to_string(marker).ok()?;
        [Self::ChargerLost, Self::Low, Self::Critical, Self::Action].into_iter().find(|alert| alert.name() == name.trim())
    }

    fn message(self, percent: u8) -> (String, &'static str) {
        match self {
            Self::ChargerLost => (
                format!("Laptop charger stopped: on battery at {percent}%"),
                "The charger or its cable dropped power while plugged in. Plug it in or reseat the cable.",
            ),
            Self::Low => (format!("Laptop battery low at {percent}%"), "The laptop is still on battery. Plug in or reseat the charger."),
            Self::Critical => (format!("Laptop battery critical at {percent}%"), Self::SHUTDOWN_WARNING),
            Self::Action => (format!("Laptop battery at {percent}%: saving memory to disk now"), Self::SHUTDOWN_WARNING),
        }
    }
}

pub fn warning_level(busctl: &mut Command) -> Option<u8> {
    let output = busctl
        .args(["get-property", "org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice", "org.freedesktop.UPower.Device", "WarningLevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout).trim().strip_prefix("u ")?.parse().ok()
}

pub fn alert_battery(marker: &Path, battery: Option<u8>, warning_level: Option<u8>, notify: &mut Command, limit: Duration) {
    let Some(percent) = battery else {
        let _ = fs::remove_file(marker);
        return;
    };
    let alert = BatteryAlert::reached(warning_level);
    if BatteryAlert::recorded(marker).is_some_and(|recorded| recorded >= alert) {
        return;
    }
    let (title, body) = alert.message(percent);
    match status_within(notify.args(["--urgency=critical", "--app-name=laptop-auto", &title, body]), limit) {
        Ok(Some(status)) if status.success() => {
            if let Err(error) = marker.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(marker, format!("{}\n", alert.name()))) {
                eprintln!("north capacity laptop-auto: {}: {error}", marker.display());
            }
        }
        Ok(Some(status)) => eprintln!("north capacity laptop-auto: battery notification exited {status}; the next tick notifies again"),
        Ok(None) => eprintln!("north capacity laptop-auto: battery notification gave no answer within {}s; the next tick notifies again", limit.as_secs()),
        Err(error) => eprintln!("north capacity laptop-auto: battery notification: {error}; the next tick notifies again"),
    }
}

fn preparing_for_sleep() -> bool {
    Command::new("busctl")
        .args(["get-property", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "PreparingForSleep"])
        .output()
        .is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "b true")
}

pub fn seat_idle() -> Option<bool> {
    let output = Command::new("loginctl")
        .args(["show-session", "auto", "-p", "IdleHint", "-p", "Type", "-p", "Active", "-p", "Remote"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let field = |name: &str| text.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix('='));
    let local = field("Active") == Some("yes") && field("Remote") == Some("no") && matches!(field("Type"), Some("wayland" | "x11"));
    local.then(|| field("IdleHint").and_then(|hint| match hint { "yes" => Some(true), "no" => Some(false), _ => None })).flatten()
}

pub fn run(arguments: &[String]) -> u8 {
    let dry = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let host = Host::from_env();
    let processes = agent_load::processes(&host);
    let battery = battery(&power_supply(), &discharging_since(&host), now());
    let idle = seat_idle();
    if dry {
        println!("{}", sample(&host, idle, preparing_for_sleep(), battery));
        return 0;
    }
    let applied = super::mode::tick(&host, idle);
    applied.lines.iter().for_each(|line| println!("{line}"));
    let sample = sample(&host, idle, preparing_for_sleep(), battery);
    shed(&host, on_battery(applied.effects, battery), &processes);
    let mut laptop = match std::env::var_os("LAPTOP_AUTO_LAPTOP") {
        Some(program) => Command::new(program),
        None => {
            let mut north = Command::new(std::env::current_exe().unwrap_or_else(|_| "north".into()));
            north.arg("laptop");
            north
        }
    };
    let observed = observe(laptop.arg("observe").arg(sample.to_string()), OBSERVE_WAIT);
    let notify = std::env::var_os("LAPTOP_AUTO_NOTIFY").unwrap_or_else(|| "notify-send".into());
    let busctl = std::env::var_os("LAPTOP_AUTO_BUSCTL").unwrap_or_else(|| "busctl".into());
    let level = battery.and_then(|_| warning_level(&mut Command::new(busctl)));
    alert_battery(&host.capacity.join(BATTERY_ALERTED), battery, level, &mut Command::new(notify), NOTIFY_WAIT);
    observed
}

pub fn observe(command: &mut Command, limit: Duration) -> u8 {
    match status_within(command, limit) {
        Ok(Some(status)) if status.success() => 0,
        Ok(Some(status)) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
        Ok(None) => {
            eprintln!("north capacity laptop-auto: laptop observe gave no answer within {}s; the next tick observes again", limit.as_secs());
            1
        }
        Err(error) => {
            eprintln!("north capacity laptop-auto: laptop observe: {error}");
            1
        }
    }
}
