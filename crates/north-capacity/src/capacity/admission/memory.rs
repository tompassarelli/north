//! Memory headroom and capacity: the floor a profile leaves free, the request it admits, and the MiB a
//! dispatched box asks of its host.

use super::super::pressure_target;
use super::{Class, Facts, Profile, Reason, default_mib};

pub(super) fn memory(facts: &Facts) -> Option<Reason> {
    if facts.memory_available_mib - facts.requested_memory_mib - pressure_target::growing_memory_mib(facts) < memory_floor_mib(facts.profile, facts.memory_total_mib) {
        return Some(Reason::DeferMemoryHeadroom);
    }
    (pressure_target::host_pressure(facts).is_none() && (facts.leased_memory_mib + facts.requested_memory_mib) * 4.0 > facts.memory_total_mib * 3.0)
        .then_some(Reason::DeferMemoryCapacity)
}

pub(crate) fn memory_floor_mib(profile: Profile, total: f64) -> f64 {
    let share = match profile {
        Profile::Attended => total / 5.0,
        Profile::Unattended => total / 12.0,
    };
    if total < 32768.0 { share } else { share.max(8192.0).min(total / 4.0) }
}

/// Whether an unattended host of `total` MiB, idle, admits a request of `requested` MiB.
pub fn memory_admits(total: f64, requested: f64) -> bool {
    !memory_short(total, total, requested) && requested * 4.0 <= total * 3.0
}

/// The MiB a dispatched box of `class` asks of its host: the heavy request for a test-verified box,
/// else the agent one.
pub fn dispatch_request_mib(class: Class) -> f64 {
    match class {
        Class::Heavy => default_mib("dispatch_heavy_memory_mib"),
        _ => default_mib("dispatch_agent_memory_mib"),
    }
}

/// Whether unattended memory headroom defers a request of `requested` MiB now.
pub fn memory_short(total: f64, available: f64, requested: f64) -> bool {
    available - requested < memory_floor_mib(Profile::Unattended, total)
}
