use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use serde_json::json;

use super::admission::{self, Facts};
use super::effects::Host;
use super::{agent_load, aperture, mode, native_hold};

fn wisp_pair_queue() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    std::env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()).map_or_else(|| home.join(".local/state"), std::path::PathBuf::from).join("wisp/lan/queue")
}

/// Admission on the caller's facts, filled in with what this host observes: game reserve, run queue, disk, Warcraft clients, aperture and native holds.
pub fn admit_enriched(mut facts: Facts, host: &Host) -> admission::Admission {
    if facts.game_reserve_agent_cpus.is_none() {
        facts.game_reserve_agent_cpus = mode::confined_agent_cpus(&agent_load::Host::from_env());
    }
    facts.gaming |= facts.game_reserve_agent_cpus.is_some() || agent_load::gaming_marked(&agent_load::capacity_root());
    facts.run_queue_load = facts.run_queue_load.or_else(|| run_queue_load(facts.cpu_some_avg10_basis_points));
    let disk_measured = facts.disk_free_gib.is_none();
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
    facts.pair_leases.extend(native_hold::queue_pair_leases(&wisp_pair_queue(), &super::local_machine::host_name()));
    facts.need = facts.need.or_else(|| std::env::var("AGENT_CAPACITY_NEED").ok().filter(|need| !need.is_empty()));
    facts.within_exclusive = std::env::var("AGENT_CAPACITY_LEASE").is_ok_and(|lease| facts.exclusive_leases.contains(&lease));
    let admitted = admission::admit(&facts, host.serving);
    let admitted = if disk_measured { reclaim_before_disk_defer(&mut facts, admitted, host) } else { admitted };
    if let Some(ticket) = facts.ticket.as_deref().filter(|_| admitted.reason != admission::Reason::Run) {
        note_reason(ticket, admitted.reason);
    }
    admitted
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

/// Gives a DEFER_DISK_FREE one reclaim and decides again on the free space it leaves; every other admission passes through.
pub fn reclaim_before_disk_defer(facts: &mut Facts, admitted: admission::Admission, host: &Host) -> admission::Admission {
    if admitted.reason != admission::Reason::DeferDiskFree {
        return admitted;
    }
    match (host.reclaim_disk)() {
        Some(free) => {
            facts.disk_free_gib = Some(free);
            admission::admit(facts, host.serving)
        }
        None => admitted,
    }
}

pub fn root_disk_free_gib() -> Option<f64> {
    if let Some(fixture) = std::env::var("AGENT_CAPACITY_DISK_FREE_GIB").ok().and_then(|gib| gib.parse().ok()) {
        return Some(fixture);
    }
    let root = std::env::var_os("AGENT_CAPACITY_DISK_ROOT").unwrap_or_else(|| "/".into());
    let stats = rustix::fs::statvfs(std::path::Path::new(&root)).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize) as f64 / f64::from(1u32 << 30))
}

const CALM_CPU_SOME_AVG10_BASIS_POINTS: u32 = 100;

fn run_queue_load(cpu_some_avg10_basis_points: u32) -> Option<f64> {
    let path = std::env::var_os("AGENT_CAPACITY_LOADAVG").unwrap_or_else(|| "/proc/loadavg".into());
    sustained_load(&std::fs::read_to_string(path).ok()?, cpu_some_avg10_basis_points)
}

pub fn sustained_load(loadavg: &str, cpu_some_avg10_basis_points: u32) -> Option<f64> {
    let mut averages = loadavg.split_whitespace().map(|field| field.parse::<f64>().ok());
    let one = averages.next()??;
    if cpu_some_avg10_basis_points < CALM_CPU_SOME_AVG10_BASIS_POINTS {
        return Some(one);
    }
    Some(averages.next().flatten().map_or(one, |five| one.max(five)))
}
