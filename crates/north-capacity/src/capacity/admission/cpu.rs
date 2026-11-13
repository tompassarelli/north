//! The host's CPU ceiling: the aggregate its profile and agent.slice set, and what a batch request may
//! use once the game reserve takes its agent slice.

use super::super::cpus;
use super::{Facts, Profile};

pub(crate) fn aggregate_cpus(profile: Profile, cores: u32, slice_cpus: Option<f64>) -> f64 {
    let name = match profile {
        Profile::Attended => "attended",
        Profile::Unattended => "unattended",
    };
    cpus::aggregate_cpus(name, f64::from(cores), slice_cpus)
}

pub(crate) fn batch_cpus(facts: &Facts) -> f64 {
    let all = aggregate_cpus(facts.profile, facts.cores, facts.slice_cpus);
    facts.game_reserve_agent_cpus.map_or(all, |agent| all.min(f64::from(agent)))
}
