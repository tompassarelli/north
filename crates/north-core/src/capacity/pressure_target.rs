//! Heavy builds start while measured CPU pressure sits under the band's high mark; the batch slice is idle-weighted,
//! so declared CPUs reserve nothing. One build starts per settle window, so the pressure read next includes the build
//! started last, and the memory of a build still growing counts against what is available. The low mark is where the
//! laptop tick thaws a batch slice it froze at the high mark.
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
    (pressure >= band(facts).high_basis_points || starting(facts)).then_some(Reason::DeferCpuPressure)
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
