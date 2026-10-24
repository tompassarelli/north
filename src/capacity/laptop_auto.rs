use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use super::agent_load::{self, Host, Process};
use crate::platform::process::status_within;

const USAGE: &str = "usage: north capacity laptop-auto [--dry-run]";
const OBSERVE_WAIT: Duration = Duration::from_secs(40);
const NOTIFY_WAIT: Duration = Duration::from_secs(5);
const BATTERY_WARNED: &str = "battery-warned";
const BATTERY_DRAIN_PERCENT: u8 = 30;

pub(crate) fn sample(host: &Host, processes: &[Process], drm: &Path, idle: Option<bool>, battery: Option<u8>) -> Value {
    let agent_gpu = processes.iter().filter(|process| process.agent_owned()).any(|process| uses_gpu(&host.proc, process.pid));
    let busy = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|card| card.file_name().to_string_lossy().starts_with("card"))
        .filter_map(|card| fs::read_to_string(card.path().join("device/gpu_busy_percent")).ok()?.trim().parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .max();
    json!({
        "gaming": agent_load::tom_gaming(processes, busy),
        "agent_gpu": agent_gpu,
        "gpu_busy": busy,
        "idle": idle,
        "battery": battery,
        "battery_low": battery_low(battery),
    })
}

pub(crate) fn discharging_percent(power_supply: &Path) -> Option<u8> {
    let read = |supply: &Path, field: &str| fs::read_to_string(supply.join(field)).ok().map(|value| value.trim().to_owned());
    fs::read_dir(power_supply)
        .into_iter()
        .flatten()
        .flatten()
        .map(|supply| supply.path())
        .filter(|supply| read(supply, "type").as_deref() == Some("Battery") && read(supply, "scope").as_deref() != Some("Device"))
        .filter(|supply| read(supply, "status").as_deref() == Some("Discharging"))
        .filter_map(|supply| read(&supply, "capacity")?.parse::<u8>().ok())
        .filter(|percent| *percent <= 100)
        .min()
}

fn battery_low(battery: Option<u8>) -> bool {
    battery.is_some_and(|percent| percent < BATTERY_DRAIN_PERCENT)
}

pub(crate) fn warn_battery_once(marker: &Path, battery: Option<u8>, notify: &mut Command, limit: Duration) {
    let Some(percent) = battery else {
        let _ = fs::remove_file(marker);
        return;
    };
    if !battery_low(battery) || marker.exists() {
        return;
    }
    let title = format!("Laptop battery at {percent}% and discharging");
    let body = "Agent placement is draining off the laptop. Plug in AC.";
    match status_within(notify.args(["--urgency=critical", "--app-name=laptop-auto", &title, body]), limit) {
        Ok(Some(status)) if status.success() => {
            if let Err(error) = marker.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(marker, format!("{percent}\n"))) {
                eprintln!("north capacity laptop-auto: {}: {error}", marker.display());
            }
        }
        Ok(Some(status)) => eprintln!("north capacity laptop-auto: battery notification exited {status}; the next tick notifies again"),
        Ok(None) => eprintln!("north capacity laptop-auto: battery notification gave no answer within {}s; the next tick notifies again", limit.as_secs()),
        Err(error) => eprintln!("north capacity laptop-auto: battery notification: {error}; the next tick notifies again"),
    }
}

fn uses_gpu(proc: &Path, pid: i32) -> bool {
    fs::read_dir(proc.join(pid.to_string()).join("fd"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|fd| fs::read_link(fd.path()).is_ok_and(|target| target.starts_with("/dev/dri/")))
}

fn seat_idle() -> Option<bool> {
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

pub(crate) fn run(arguments: &[String]) -> u8 {
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
    let drm = std::env::var_os("AGENT_LOAD_DRM").unwrap_or_else(|| "/sys/class/drm".into());
    let power_supply = std::env::var_os("AGENT_LOAD_POWER_SUPPLY").unwrap_or_else(|| "/sys/class/power_supply".into());
    let battery = discharging_percent(Path::new(&power_supply));
    let sample = sample(&host, &processes, Path::new(&drm), seat_idle(), battery);
    if dry {
        println!("{sample}");
        return 0;
    }
    let gaming = sample["gaming"] == true || agent_load::game_reserve_on(&host);
    let plan = agent_load::enforce(&host, gaming, &processes);
    if plan != agent_load::Plan::default() {
        println!(
            "agent-load: gaming={gaming} stopped {:?} froze {:?} thawed {:?} killed {:?}",
            plan.stop, plan.freeze, plan.thaw, plan.kill
        );
    }
    let laptop = std::env::var_os("LAPTOP_AUTO_LAPTOP").unwrap_or_else(|| "laptop".into());
    let observed = observe(Command::new(laptop).arg("observe").arg(sample.to_string()), OBSERVE_WAIT);
    let notify = std::env::var_os("LAPTOP_AUTO_NOTIFY").unwrap_or_else(|| "notify-send".into());
    warn_battery_once(&host.capacity.join(BATTERY_WARNED), battery, &mut Command::new(notify), NOTIFY_WAIT);
    observed
}

pub(crate) fn observe(command: &mut Command, limit: Duration) -> u8 {
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
