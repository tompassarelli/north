use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use super::agent_load::{self, Host, Process};

const USAGE: &str = "usage: north capacity laptop-auto [--dry-run]";

pub(crate) fn sample(host: &Host, processes: &[Process], drm: &Path, idle: Option<bool>) -> Value {
    let agent_gpu = processes.iter().filter(|process| process.agent_owned()).any(|process| uses_gpu(&host.proc, process.pid));
    let busy = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|card| card.file_name().to_string_lossy().starts_with("card"))
        .filter_map(|card| fs::read_to_string(card.path().join("device/gpu_busy_percent")).ok()?.trim().parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .max();
    json!({ "gaming": agent_load::tom_gaming(processes, busy), "agent_gpu": agent_gpu, "gpu_busy": busy, "idle": idle })
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
    let sample = sample(&host, &processes, Path::new(&drm), seat_idle());
    if dry {
        println!("{sample}");
        return 0;
    }
    let gaming = sample["gaming"] == true || agent_load::playtest_on(&host);
    let plan = agent_load::enforce(&host, gaming, &processes);
    if plan != agent_load::Plan::default() {
        println!(
            "agent-load: gaming={gaming} stopped {:?} froze {:?} thawed {:?} killed {:?}",
            plan.stop, plan.freeze, plan.thaw, plan.kill
        );
    }
    let laptop = std::env::var_os("LAPTOP_AUTO_LAPTOP").unwrap_or_else(|| "laptop".into());
    match Command::new(laptop).arg("observe").arg(sample.to_string()).status() {
        Ok(status) if status.success() => 0,
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
        Err(error) => {
            eprintln!("north capacity laptop-auto: laptop observe: {error}");
            1
        }
    }
}
