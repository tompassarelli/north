//! Batch requests: exclusive and critical classes, CPU pressure, batch caps and the shared CPU room a heavy
//! or moderate run gets last.

use super::super::{pressure_target, queued_ahead};
use super::cpu::batch_cpus;
use super::gpu::{gpu_memory_short, gpu_slots};
use super::native::fencing_owners;
use super::{Class, Facts, Profile, Reason, default_mib};

const INTERACTIVE_PRESSURE_BASIS_POINTS: u32 = 1000;
const BATCH_PRESSURE_BASIS_POINTS: u32 = 3000;

pub(super) fn batch(facts: &Facts) -> Reason {
    if let Some(reason) = held_by_peers(facts) {
        return reason;
    }
    if matches!(facts.class, Class::Critical | Class::Tom | Class::Runtime) {
        return if interactive_pressure(facts) { Reason::DeferInteractivePressure } else { Reason::Run };
    }
    if !facts.beside_native && !fencing_owners(facts).is_empty() {
        return Reason::DeferNativeQuiet;
    }
    if let Some(reason) = by_class(facts) {
        return reason;
    }
    shared(facts)
}

/// The peers whose runs defer every batch request: an unbounded peer, an exclusive peer, and a land
/// check whose charge would overfill the batch limit.
fn held_by_peers(facts: &Facts) -> Option<Reason> {
    if facts.unbounded_runs > 0 {
        return Some(Reason::DeferUnboundedPeer);
    }
    if facts.peer_exclusive_runs > 0 {
        return Some(Reason::DeferExclusive);
    }
    (facts.class == Class::Critical && landing_cpus_full(facts)).then_some(Reason::DeferCpuCapacity)
}

/// The class's own gate: a GPU run waits for native work, slots and VRAM, and an exclusive request waits
/// for the queue and its peers.
fn by_class(facts: &Facts) -> Option<Reason> {
    match facts.class {
        Class::Gpu => {
            if facts.native_waiting > 0 {
                return Some(Reason::DeferNativeWaiting);
            }
            if facts.gpu_runs >= gpu_slots(facts.host_gpu_slots) {
                return Some(Reason::DeferGpuSlots);
            }
            gpu_memory_short(facts).then_some(Reason::DeferGpuMemory)
        }
        Class::Exclusive => Some(if facts.exclusive_waiting > 0 {
            Reason::DeferQueued
        } else if facts.peer_batch_runs > 0 {
            Reason::DeferExclusive
        } else {
            Reason::Run
        }),
        _ => None,
    }
}

fn shared(facts: &Facts) -> Reason {
    if facts.exclusive_waiting > 0 && facts.class == Class::Heavy && !facts.before_drain {
        return Reason::DeferExclusiveQueued;
    }
    if interactive_pressure(facts) {
        return Reason::DeferInteractivePressure;
    }
    if queued_ahead::queued_ahead(facts) > 0 {
        return Reason::DeferQueued;
    }
    if facts.class == Class::Gpu {
        return Reason::Run;
    }
    cpu_room(facts)
}

fn cpu_room(facts: &Facts) -> Reason {
    let by_host_pressure = pressure_target::host_pressure(facts).is_some();
    if !by_host_pressure && batch_jobs_full(facts) {
        return Reason::DeferBatchJobs;
    }
    if !by_host_pressure && facts.peer_batch_runs > 0 && !batch_fits(facts, facts.requested_cpus) {
        return Reason::DeferCpuCapacity;
    }
    if facts.class == Class::Heavy {
        return pressure_target::deferred(facts).unwrap_or(Reason::Run);
    }
    if facts.cpu_some_avg10_basis_points > BATCH_PRESSURE_BASIS_POINTS || pressure_target::host_over(facts) {
        return Reason::DeferCpuPressure;
    }
    Reason::Run
}

pub(crate) fn batch_jobs_cap(cores: u32) -> u32 {
    ((f64::from(cores) * default_mib("admit_batch_jobs_per_core")).floor() as u32).max(1)
}

fn batch_jobs_full(facts: &Facts) -> bool {
    let running = facts.leases.iter().filter(|lease| matches!(lease.class.as_str(), "heavy" | "moderate")).count();
    u32::try_from(running).unwrap_or(u32::MAX) >= batch_jobs_cap(facts.cores)
}

pub(crate) fn batch_fits(facts: &Facts, cpus: f64) -> bool {
    facts.leased_batch_cpus + facts.unleased_heavy_cores + cpus <= batch_cpus(facts)
}

/// Whether the land checks already running, with this one, charge more CPUs than the host's batch limit: a lease scope is held to its charge by CPUQuota, so a second check beside the first runs only while both fit, and a lone check always runs. A runtime canary's check takes a reserve of one canary check outside that sum, so a host whose land checks overfill it still answers its canary, which gex45 did not on 2026-11-13 under 29 leased batch CPUs of 20.
fn landing_cpus_full(facts: &Facts) -> bool {
    if facts.canary {
        return facts.leases.iter().any(|lease| lease.canary);
    }
    let landing = || facts.leases.iter().filter(|lease| lease.landing && !lease.canary);
    landing().next().is_some() && landing().map(|lease| lease.cpus).sum::<f64>() + facts.requested_cpus > batch_cpus(facts)
}

fn interactive_pressure(facts: &Facts) -> bool {
    facts.profile == Profile::Attended && facts.protected_cpu_some_avg10_basis_points >= INTERACTIVE_PRESSURE_BASIS_POINTS
}
