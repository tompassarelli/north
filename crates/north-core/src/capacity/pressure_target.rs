//! Heavy builds start while measured CPU pressure sits under a target band, not while a job count has room.
//! Below the band's low mark the next queued build starts at once; between the marks it starts only if its declared
//! CPUs fit the batch cores; at the high mark nothing starts. One build starts per settle window, so the pressure
//! read next includes the build started last, and the memory of a build still growing counts against what is available.
use super::admission::{Class, Facts, Profile, Reason, batch_cpus, default_mib};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Band {
    pub(super) low_basis_points: f64,
    pub(super) high_basis_points: f64,
}

pub(super) fn band(facts: &Facts) -> Band {
    let (low, high) = match (facts.game_reserve_agent_cpus.is_some(), facts.profile) {
        (true, _) => ("admit_psi_game_low", "admit_psi_game_high"),
        (false, Profile::Attended) => ("admit_psi_attended_low", "admit_psi_attended_high"),
        (false, Profile::Unattended) => ("admit_psi_unattended_low", "admit_psi_unattended_high"),
    };
    Band { low_basis_points: default_mib(low) * 100.0, high_basis_points: default_mib(high) * 100.0 }
}

/// Why a heavy request waits on CPU, the first of: a build started inside the settle window, declared CPUs past the
/// overcommit rail, pressure at the band's high mark, or pressure in the band with the request not fitting the batch cores.
pub(super) fn deferred(facts: &Facts) -> Option<Reason> {
    if facts.class != Class::Heavy {
        return None;
    }
    let pressure = f64::from(facts.cpu_some_avg10_basis_points);
    let band = band(facts);
    if pressure >= band.high_basis_points || starting(facts) {
        return Some(Reason::DeferCpuPressure);
    }
    let committed = facts.leased_batch_cpus + facts.unleased_heavy_cores + facts.requested_cpus;
    let cores = batch_cpus(facts);
    if committed > cores * default_mib(rail(facts)) {
        return Some(Reason::DeferCpuCapacity);
    }
    (pressure >= band.low_basis_points && committed > cores).then_some(Reason::DeferCpuPressure)
}

fn rail(facts: &Facts) -> &'static str {
    match (facts.game_reserve_agent_cpus.is_some(), facts.profile) {
        (true, _) => "admit_overcommit_game",
        (false, Profile::Attended) => "admit_overcommit_attended",
        (false, Profile::Unattended) => "admit_overcommit_unattended",
    }
}

fn age_seconds(facts: &Facts, created_at: f64) -> f64 {
    (facts.now - created_at) / 1000.0
}

fn heavy_leases(facts: &Facts) -> impl Iterator<Item = &super::native_hold::LeaseFact> {
    facts.leases.iter().filter(|lease| matches!(lease.class.as_str(), "heavy" | "runtime"))
}

fn starting(facts: &Facts) -> bool {
    let settle = std::env::var("AGENT_CAPACITY_SETTLE_SECONDS").ok().and_then(|seconds| seconds.parse().ok()).unwrap_or_else(|| default_mib("admit_settle_seconds"));
    heavy_leases(facts).any(|lease| age_seconds(facts, lease.created_at) < settle)
}

/// The MiB of heavy leases that started within the ramp and have not yet reached MemAvailable.
pub(super) fn growing_memory_mib(facts: &Facts) -> f64 {
    if facts.class != Class::Heavy {
        return 0.0;
    }
    let ramp = default_mib("admit_memory_ramp_seconds");
    heavy_leases(facts).filter(|lease| age_seconds(facts, lease.created_at) < ramp).map(|lease| lease.memory_mib).sum()
}
