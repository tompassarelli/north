//! A native request's place beside interactive work: quiet fences, alone runs, waiters ahead and the GPU
//! clients that hold slots.

use super::super::native_hold;
use super::cpu::aggregate_cpus;
use super::gpu::{GPU_BUSY_PERCENT, over_client_cap};
use super::{Class, Facts, Reason};

pub(super) fn fencing_owners(facts: &Facts) -> Vec<String> {
    native_hold::fencing_owners(&facts.quiet_native, native_hold::quiet_idle_bound(), facts.class == Class::Heavy, facts.owner.as_deref())
}

/// Native waiters ahead in line, less those whose own clients would defer as DEFER_GPU_CLIENTS: they hold no place, so a smaller start behind them takes a slot that frees.
fn native_queued_ahead(facts: &Facts) -> u32 {
    if facts.queued_ahead_new_clients.is_empty() {
        return facts.queued_ahead;
    }
    let holding = facts.queued_ahead_new_clients.iter().filter(|new_clients| !over_client_cap(facts, **new_clients, facts.client_hold.held)).count();
    u32::try_from(holding).unwrap_or(u32::MAX)
}

pub(super) fn native(facts: &Facts) -> Reason {
    if let Some(reason) = fenced_or_queued(facts) {
        return reason;
    }
    if let Some(reason) = over_native_limits(facts) {
        return reason;
    }
    Reason::Run
}

/// The line a native request waits in: a quiet fence, an alone request, native waiters ahead, and an
/// exclusive peer it may not join.
fn fenced_or_queued(facts: &Facts) -> Option<Reason> {
    if !facts.beside_native && !fencing_owners(facts).is_empty() {
        return Some(Reason::DeferNativeQuiet);
    }
    if native_hold::alone_defers(facts.alone, facts.alone_queued_ahead, facts.gpu_clients) {
        return Some(Reason::DeferNativeAlone);
    }
    if (facts.new_clients > 0 || facts.new_desktops > 0) && native_queued_ahead(facts) > 0 {
        return Some(Reason::DeferQueued);
    }
    (facts.quiet && facts.peer_exclusive_runs > u32::from(facts.within_exclusive)).then_some(Reason::DeferExclusive)
}

/// What the request asks beside what the host runs: native CPUs, reserved client slots and GPU busy.
fn over_native_limits(facts: &Facts) -> Option<Reason> {
    if facts.leased_native_cpus + facts.requested_cpus > aggregate_cpus(facts.profile, facts.cores, facts.slice_cpus) {
        return Some(Reason::DeferNativeCpus);
    }
    if over_client_cap(facts, facts.new_clients, facts.client_hold.held - facts.client_hold.drawn) {
        return Some(Reason::DeferGpuClients);
    }
    (facts.gpu_busy_percent >= GPU_BUSY_PERCENT && facts.gpu_runs == 0).then_some(Reason::DeferGpuBusy)
}
