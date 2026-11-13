//! The order admission weighs its subjects: validity, pairs, land checks, gaming, memory, disk, the run
//! queue, GPU measures and then the request's own class.

use super::super::native_hold;
use super::super::{gpu_admission, land_claim, run_queue};
use super::batch::batch;
use super::disk::{disk_below_floor, disk_short};
use super::gaming::{game_memory_short, gaming_deferred};
use super::memory::memory;
use super::native::native;
use super::{Class, Facts, Reason};

pub fn decide(facts: &Facts) -> Reason {
    if let Some(reason) = deferred_before_class(facts) {
        return reason;
    }
    match facts.class {
        Class::Agent => Reason::Run,
        Class::Native => native(facts),
        _ => batch(facts),
    }
}

/// The deferrals every class shares, in the order they run: a broken request, a pair hold, a land
/// check's claim, the game reserve, memory, disk, the run queue and the GPU's own measures.
fn deferred_before_class(facts: &Facts) -> Option<Reason> {
    if invalid(facts) {
        return Some(Reason::Invalid);
    }
    if native_hold::pair_refusal(facts.pair, facts.owner.as_deref(), &facts.pair_leases).is_some() {
        return Some(Reason::PairHeld);
    }
    if facts.land_check_waiting && land_claim::yields(facts.class, facts.holds_offload_slot) {
        return Some(Reason::DeferLandCheckWaiting);
    }
    if facts.gaming && gaming_deferred(facts, facts.gaming_class.unwrap_or(facts.class)) {
        return Some(Reason::DeferGaming);
    }
    if game_memory_short(facts) {
        return Some(Reason::DeferGameMemory);
    }
    if let Some(reason) = memory(facts) {
        return Some(reason);
    }
    if disk_short(facts) {
        return Some(Reason::DeferDiskFree);
    }
    if run_queue::overloaded(facts) {
        return Some(Reason::DeferRunQueue);
    }
    gpu_admission::deferred(facts)
}

pub(crate) fn host_holds(facts: &Facts, class: Class) -> bool {
    (facts.gaming && gaming_deferred(facts, class)) || (class == Class::Heavy && disk_below_floor(facts))
}

fn invalid(facts: &Facts) -> bool {
    facts.cores == 0
        || facts.memory_total_mib <= 0.0
        || facts.memory_available_mib <= 0.0
        || facts.requested_cpus < 0.0
        || facts.requested_memory_mib <= 0.0
}
