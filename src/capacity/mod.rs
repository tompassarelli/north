use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

use serde_json::json;

pub(crate) mod admission;
mod aperture;
pub(crate) mod agent_load;
pub(crate) mod laptop_auto;
mod game_reserve;
mod unleased;
mod native_hold;
mod deadline;
mod lease_deadline;
mod warcraft;
#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod agent_load_tests;
#[cfg(test)]
mod game_reserve_tests;
#[cfg(test)]
mod unleased_tests;
#[cfg(test)]
mod native_hold_tests;
#[cfg(test)]
mod tests;

use admission::Facts;

pub(crate) struct Host {
    pub(crate) serving: fn(&str) -> Option<String>,
    pub(crate) warcraft_clients: fn() -> Option<u32>,
    pub(crate) bar1_margin_mib: fn() -> Option<u32>,
    pub(crate) offload: fn(&[String]) -> u8,
}

fn route(arguments: &[String], offload: fn(&[String]) -> u8) -> Option<u8> {
    let status = match arguments.first().map(String::as_str) {
        Some("admission") if arguments.len() == 1 => return None,
        Some("laptop-auto") => laptop_auto::run(&arguments[1..]),
        Some("offload") => offload(&arguments[1..]),
        Some("reap-orphans") if arguments.len() == 1 => agent_load::reap_orphans_run(),
        Some("game-reserve") => game_reserve::run(&arguments[1..]),
        Some("deadline") => deadline::run(&arguments[1..]),
        Some("warcraft-clients") if arguments.len() == 1 => warcraft::run(),
        Some("unleased") if arguments.len() == 1 => unleased::run(),
        Some("quiet") if arguments.len() == 1 => native_hold::run(admission::quiet_idle_bound()),
        _ => {
            eprintln!("usage: north capacity admission < FACTS.json | laptop-auto [--dry-run] | reap-orphans | game-reserve on [--auto]|off|status | offload choose|run | deadline LEASE_FILE SECONDS -- COMMAND... | warcraft-clients | unleased | quiet < LEASES.json");
            2
        }
    };
    Some(status)
}

pub(crate) fn run(arguments: &[String], host: Host) -> u8 {
    if let Some(status) = route(arguments, host.offload) {
        return status;
    }
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Facts>(&input).map_err(|error| error.to_string()));
    match outcome {
        Ok(mut facts) => {
            if facts.game_reserve_agent_cpus.is_none() && agent_load::game_reserve_on(&agent_load::Host::from_env()) {
                facts.game_reserve_agent_cpus = Some(game_reserve::Settings::from_env().agent_cpu_count());
            }
            facts.gaming |= facts.game_reserve_agent_cpus.is_some() || agent_load::gaming_marked(&agent_load::capacity_root());
            facts.run_queue_load = facts.run_queue_load.or_else(run_queue_load);
            facts.disk_free_gib = facts.disk_free_gib.or_else(root_disk_free_gib);
            facts.host_warcraft_clients = facts.host_warcraft_clients.or_else(host.warcraft_clients);
            facts.host_bar1_margin_mib = facts.host_bar1_margin_mib.or_else(host.bar1_margin_mib);
            if facts.bar1_free_mib.is_none() && !facts.bar1_timed_out && admission::starts_gpu_work(&facts) {
                let memory = aperture::memory();
                facts.vram_free_mib = facts.vram_free_mib.or(memory.vram_free_mib);
                match memory.bar1 {
                    aperture::Bar1::Free(free) => facts.bar1_free_mib = Some(free),
                    aperture::Bar1::TimedOut => facts.bar1_timed_out = true,
                    aperture::Bar1::Unknown => {}
                }
            }
            facts.derive_native_holds();
            facts.need = facts.need.or_else(|| std::env::var("AGENT_CAPACITY_NEED").ok().filter(|need| !need.is_empty()));
            facts.within_exclusive = std::env::var("AGENT_CAPACITY_LEASE").is_ok_and(|lease| facts.exclusive_leases.contains(&lease));
            let admitted = admission::admit(&facts, host.serving);
            if let Some(ticket) = facts.ticket.as_deref().filter(|_| admitted.reason != admission::Reason::Run) {
                note_reason(ticket, admitted.reason);
            }
            let _ = writeln!(std::io::stdout(), "{}", json!(admitted));
            0
        }
        Err(message) => {
            eprintln!("north capacity: {message}");
            2
        }
    }
}

fn note_reason(ticket: &std::path::Path, reason: admission::Reason) {
    let Some(mut entry) = std::fs::read(ticket).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes).ok()) else {
        return;
    };
    let reason = json!(reason);
    if entry.get("reason") == Some(&reason) {
        return;
    }
    entry.insert("reason".to_owned(), reason);
    let staged = ticket.with_extension("json.tmp");
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staged)
        .and_then(|mut file| writeln!(file, "{}", serde_json::Value::Object(entry)))
        .and_then(|()| std::fs::rename(&staged, ticket));
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
}

fn root_disk_free_gib() -> Option<f64> {
    if let Some(fixture) = std::env::var("AGENT_CAPACITY_DISK_FREE_GIB").ok().and_then(|gib| gib.parse().ok()) {
        return Some(fixture);
    }
    let root = std::env::var_os("AGENT_CAPACITY_DISK_ROOT").unwrap_or_else(|| "/".into());
    let stats = rustix::fs::statvfs(std::path::Path::new(&root)).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize) as f64 / f64::from(1u32 << 30))
}

fn run_queue_load() -> Option<f64> {
    let path = std::env::var_os("AGENT_CAPACITY_LOADAVG").unwrap_or_else(|| "/proc/loadavg".into());
    std::fs::read_to_string(path).ok()?.split_whitespace().next()?.parse().ok()
}

pub(crate) enum DesktopAperture {
    Fits,
    Short { free_mib: f64, need_mib: f64 },
    TimedOut,
}

pub(crate) fn desktop_aperture(host_margin_mib: Option<u32>) -> DesktopAperture {
    let need_mib = admission::aperture_need_mib(1, 0, host_margin_mib);
    match aperture::bar1() {
        aperture::Bar1::Free(free_mib) if free_mib < need_mib => DesktopAperture::Short { free_mib, need_mib },
        aperture::Bar1::TimedOut => DesktopAperture::TimedOut,
        _ => DesktopAperture::Fits,
    }
}
