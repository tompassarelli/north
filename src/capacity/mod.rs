use std::io::{Read, Write};

use serde_json::json;

pub(crate) mod admission;
mod aperture;
pub(crate) mod agent_load;
pub(crate) mod laptop_auto;
mod game_reserve;
mod deadline;
mod lease_deadline;
mod warcraft;
#[cfg(test)]
mod agent_load_tests;
#[cfg(test)]
mod game_reserve_tests;
#[cfg(test)]
mod tests;

use admission::Facts;

pub(crate) struct Host {
    pub(crate) serving: fn(&str) -> Option<String>,
    pub(crate) warcraft_clients: fn() -> Option<u32>,
    pub(crate) bar1_margin_mib: fn() -> Option<u32>,
}

pub(crate) fn run(arguments: &[String], host: Host) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("admission") if arguments.len() == 1 => {}
        Some("laptop-auto") => return laptop_auto::run(&arguments[1..]),
        Some("reap-orphans") if arguments.len() == 1 => return agent_load::reap_orphans_run(),
        Some("game-reserve") => return game_reserve::run(&arguments[1..]),
        Some("deadline") => return deadline::run(&arguments[1..]),
        Some("warcraft-clients") if arguments.len() == 1 => return warcraft::run(),
        _ => {
            eprintln!("usage: north capacity admission < FACTS.json | laptop-auto [--dry-run] | reap-orphans | game-reserve on [--auto]|off|status | deadline LEASE_FILE SECONDS -- COMMAND... | warcraft-clients");
            return 2;
        }
    }
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Facts>(&input).map_err(|error| error.to_string()));
    match outcome {
        Ok(mut facts) => {
            facts.gaming |= agent_load::gaming_marked(&agent_load::capacity_root());
            facts.run_queue_load = facts.run_queue_load.or_else(run_queue_load);
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
            facts.need = facts.need.or_else(|| std::env::var("AGENT_CAPACITY_NEED").ok().filter(|need| !need.is_empty()));
            facts.within_exclusive = std::env::var("AGENT_CAPACITY_LEASE").is_ok_and(|lease| facts.exclusive_leases.contains(&lease));
            let _ = writeln!(std::io::stdout(), "{}", json!(admission::admit(&facts, host.serving)));
            0
        }
        Err(message) => {
            eprintln!("north capacity: {message}");
            2
        }
    }
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
