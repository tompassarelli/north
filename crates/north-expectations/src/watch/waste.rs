use super::{Alert, HostView, Severity, WasteView, WatchContext};
use toml_edit::DocumentMut;

static DEFAULTS: std::sync::LazyLock<&DocumentMut> = std::sync::LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

fn threshold(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(super) const REPEAT_SECS: u64 = 15 * 60;
const GAME_IDLE_SECS: u64 = 10 * 60;
const COMMIT_NEAR_LIMIT: f64 = 0.95;
const LIVE_FAR_BELOW_COMMIT: f64 = 0.5;

struct Idle {
    waited: u64,
    waiting: usize,
}

fn idle(host: &HostView, waste: &WasteView, now: u64) -> Option<Idle> {
    let tickets: Vec<u64> = host.capacity.iter().flat_map(|view| view.queued.iter()).map(|ticket| now.saturating_sub(ticket.queued_since)).collect();
    let waited = tickets.iter().chain(&waste.offloads_waiting).copied().max()?;
    let quiet_cpu = waste.cores > 0.0 && waste.busy_cores < waste.cores * threshold("watch_waste_cpu_fraction");
    let free_memory = waste.memory_total_mib > 0.0 && waste.memory_available_mib > waste.memory_total_mib * threshold("watch_waste_memory_fraction");
    let long = waited as f64 > threshold("watch_waste_wait_seconds");
    (long && quiet_cpu && free_memory).then_some(Idle { waited, waiting: tickets.len() + waste.offloads_waiting.len() })
}

fn causes(waste: &WasteView, now: u64) -> Vec<(&'static str, String)> {
    let mut causes = Vec::new();
    if waste.game_reserve_on && waste.game_seen.is_none_or(|seen| now.saturating_sub(seen) > GAME_IDLE_SECS) {
        let seen = waste.game_seen.map_or_else(|| "no game ever seen".to_owned(), |seen| format!("no game seen for {} min", now.saturating_sub(seen) / 60));
        causes.push(("game-reserve", format!("game reserve is on with {seen}")));
    }
    for pool in waste.slot_pools.iter().filter(|pool| pool.total > 0 && pool.held == pool.total && !waste.offloads_waiting.is_empty()) {
        causes.push(("offload-slots", format!("all {} {} offload slots are held while the host idles", pool.total, pool.name)));
    }
    if waste.cpu_limit > 0.0 && waste.committed_cpus >= waste.cpu_limit * COMMIT_NEAR_LIMIT && waste.busy_cores < waste.committed_cpus * LIVE_FAR_BELOW_COMMIT {
        causes.push(("commit-limit", format!("CPU commit accounting is at {:.1} of its {:.0} limit while live use is {:.1} cores", waste.committed_cpus, waste.cpu_limit, waste.busy_cores)));
    }
    if !waste.stale_leases.is_empty() {
        causes.push(("stale-lease", format!("leases holding CPUs with zero live cores for over an hour: {}", waste.stale_leases.join(", "))));
    }
    if causes.is_empty() {
        causes.push(("unexplained", "no cause the probe can see".to_owned()));
    }
    causes
}

pub fn waste(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for host in &context.hosts {
        let Some(waste) = &host.waste else { continue };
        let Some(idle) = idle(host, waste, context.now) else { continue };
        let free = 100.0 * waste.memory_available_mib / waste.memory_total_mib;
        let scene = format!("{} waiting up to {} s on {} while {:.1} of {:.0} cores are busy and {free:.0}% of memory is free", idle.waiting, idle.waited, host.name, waste.busy_cores, waste.cores);
        for (cause, text) in causes(waste, context.now) {
            alerts.push(Alert {
                key: format!("work:waste:{}:{cause}", host.name),
                severity: Severity::Page,
                repo: None,
                owner: None,
                summary: format!("{scene}: {text}"),
                since: context.now.saturating_sub(idle.waited),
                repeat_secs: Some(REPEAT_SECS),
            });
        }
    }
    alerts
}
