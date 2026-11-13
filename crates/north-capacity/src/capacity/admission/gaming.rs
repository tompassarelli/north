//! The game reserve: which classes a gaming host defers, and the memory a reserved agent slice keeps free.

use super::{Class, Facts, default_mib};

pub(super) fn gaming_deferred(facts: &Facts, class: Class) -> bool {
    match class {
        Class::Native | Class::Exclusive | Class::Gpu => true,
        Class::Heavy => facts.game_reserve_agent_cpus.is_none(),
        _ => false,
    }
}

pub(super) fn game_memory_short(facts: &Facts) -> bool {
    facts.game_reserve_agent_cpus.is_some()
        && facts.class != Class::Agent
        && facts.memory_available_mib - facts.requested_memory_mib < default_mib("game_reserve_headroom_mib")
}
