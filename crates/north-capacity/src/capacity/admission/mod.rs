//! Machine admission: the facts a host sends, the verdict it gets back, and the subjects each verdict weighs.
//! `admit` turns the facts into a verdict, `decision` fixes the order the subjects run in, and each subject
//! module owns the measures it uses.

use super::client_reserve;
use super::native_hold;

mod batch;
mod cpu;
mod decision;
mod disk;
mod facts;
mod gaming;
mod gpu;
mod memory;
mod native;
mod verdict;

pub use super::defaults::default_mib;
pub use super::native_hold::quiet_idle_bound;
pub use decision::decide;
pub use facts::{AheadTicket, Class, Facts, Profile};
pub use gpu::{aperture_need_mib, gpu_slots, starts_gpu_work, warcraft_client_cap};
pub use memory::{dispatch_request_mib, memory_admits, memory_short};
pub use verdict::{Admission, Aperture, Reason};
pub(crate) use memory::memory_floor_mib;
pub(super) use batch::batch_fits;
pub(super) use decision::host_holds;
pub(super) use gpu::{aperture_short, GPU_BUSY_PERCENT};

pub fn admit(facts: &Facts, serving: impl Fn(&str) -> Option<String>) -> Admission {
    let reason = decide(facts);
    let offload = match (reason, facts.need.as_deref()) {
        (Reason::DeferRunQueue | Reason::DeferGpuBusy, Some(needs)) => serving(needs),
        _ => None,
    };
    let aperture = (reason == Reason::DeferGpuAperture)
        .then(|| facts.bar1_free_mib.map(|bar1_free_mib| Aperture { bar1_free_mib, bar1_need_mib: gpu::bar1_need_mib(facts) }))
        .flatten();
    let disk_free_gib = (reason == Reason::DeferDiskFree).then_some(facts.disk_free_gib).flatten();
    let pair = native_hold::pair_refusal(facts.pair, facts.owner.as_deref(), &facts.pair_leases).filter(|_| reason == Reason::PairHeld);
    let quiet_owners = if reason == Reason::DeferNativeQuiet { native::fencing_owners(facts) } else { Vec::new() };
    Admission { reason, offload, aperture, disk_free_gib, pair, quiet_owners }
}

impl Facts {
    /// Fills the quiet, pair, alone and reserved-client facts from the raw leases and queue the host passed.
    pub fn derive_native_holds(&mut self) {
        let ticket = self.ticket.as_deref().and_then(|path| path.file_name()).and_then(|name| name.to_str());
        let derived = native_hold::derive(&self.leases, &self.queue, ticket, self.now);
        (self.quiet_native, self.pair_leases, self.alone_queued_ahead) = (derived.quiet_native, derived.pair_leases, derived.alone_queued_ahead);
        self.client_hold = client_reserve::client_hold(&self.leases, self.requester_lease.as_deref(), self.owner.as_deref(), self.new_clients, self.now, default_mib("client_reserve_seconds") * 1000.0);
    }
}

pub fn offload_room() -> (f64, f64) {
    (default_mib("land_offload_cpus"), default_mib("land_offload_disk_free_gib"))
}
