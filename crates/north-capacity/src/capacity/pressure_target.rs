//! Heavy builds start while measured CPU pressure sits under the band's high mark; the batch slice is idle-weighted,
//! so declared CPUs reserve nothing. Off a build host one build starts per settle window, so the pressure read next
//! includes the build started last; a build host waits on its whole host's pressure alone, as its offloaded jobs do.
//! The memory of a build still growing counts against what is available. The low mark is where the laptop tick thaws
//! a batch slice it froze at the high mark.
use super::admission::{Class, Facts, Profile, Reason, default_mib};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub low_basis_points: f64,
    pub high_basis_points: f64,
}

pub(super) fn band(facts: &Facts) -> Band {
    band_for(facts.game_reserve_agent_cpus.is_some(), facts.profile)
}

pub fn band_for(game_reserve: bool, profile: Profile) -> Band {
    let (low, high) = match (game_reserve, profile) {
        (true, _) => ("admit_psi_game_low", "admit_psi_game_high"),
        (false, Profile::Attended) => ("admit_psi_attended_low", "admit_psi_attended_high"),
        (false, Profile::Unattended) => ("admit_psi_unattended_low", "admit_psi_unattended_high"),
    };
    Band { low_basis_points: default_mib(low) * 100.0, high_basis_points: default_mib(high) * 100.0 }
}

pub(super) fn deferred(facts: &Facts) -> Option<Reason> {
    if facts.class != Class::Heavy {
        return None;
    }
    let pressure = f64::from(facts.cpu_some_avg10_basis_points);
    (pressure >= band(facts).high_basis_points || host_over(facts) || (host_pressure(facts).is_none() && starting(facts))).then_some(Reason::DeferCpuPressure)
}

fn age_seconds(facts: &Facts, created_at: f64) -> f64 {
    (facts.now - created_at) / 1000.0
}

fn heavy_leases(facts: &Facts) -> impl Iterator<Item = &super::native_hold::LeaseFact> {
    facts.leases.iter().filter(|lease| matches!(lease.class.as_str(), "heavy" | "runtime"))
}

/// The seconds after a heavy build starts before another starts.
pub fn settle_seconds() -> f64 {
    std::env::var("AGENT_CAPACITY_SETTLE_SECONDS").ok().and_then(|seconds| seconds.parse().ok()).unwrap_or_else(|| default_mib("admit_settle_seconds"))
}

/// Landing leases start past this window, and their load reaches admission through the pressure reading, so their starts open none: a stream of land checks would otherwise hold every queued build while pressure reads 0.
fn starting(facts: &Facts) -> bool {
    settling_seconds(heavy_leases(facts).filter(|lease| !lease.landing).map(|lease| lease.created_at), facts.now) > 0.0
}

/// The seconds until the youngest of the heavy builds started at `started_ms` leaves its settle window, when admission starts the next; 0 when none is settling.
pub fn settling_seconds(started_ms: impl IntoIterator<Item = f64>, now_ms: f64) -> f64 {
    let settle = settle_seconds();
    started_ms.into_iter().map(|started| settle - (now_ms - started) / 1000.0).fold(0.0, f64::max)
}

/// The MiB of heavy leases that started within the ramp and have not yet reached MemAvailable.
pub(super) fn growing_memory_mib(facts: &Facts) -> f64 {
    if facts.class != Class::Heavy {
        return 0.0;
    }
    let ramp = default_mib("admit_memory_ramp_seconds");
    heavy_leases(facts).filter(|lease| age_seconds(facts, lease.created_at) < ramp).map(|lease| lease.memory_mib).sum()
}

/// On a build host the whole host's CPU pressure, batch slices included, holds a heavy or moderate run against `offload_job_cpu_psi` in place of the CPUs and memory its peers reserve; a land check there waits on neither sum.
pub(super) fn host_pressure(facts: &Facts) -> Option<u32> {
    (matches!(facts.class, Class::Heavy | Class::Moderate | Class::Critical) && facts.build_host).then_some(facts.host_cpu_some_avg10_basis_points).flatten()
}

pub(super) fn host_over(facts: &Facts) -> bool {
    host_pressure(facts).is_some_and(|host| f64::from(host) >= default_mib("offload_job_cpu_psi") * 100.0)
}
