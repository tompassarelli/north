//! Whether a build host starts another offloaded job (a lane's pre-queue check, a worker build, the slot warmer's build) now: while the
//! whole host's CPU pressure, which counts the jobs already running in its batch slices, stays under `offload_job_cpu_psi` and its
//! available memory covers one more job's measured peak and the host's floor. The lander's own jobs never ask; they keep a floor.
use super::admission::{default_mib, memory_short};

/// The host's readings one decision takes: `/proc/pressure/cpu` some avg10 in percent and `/proc/meminfo` in MiB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostLoad {
    pub cpu_some_percent: f64,
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
}

/// Why the host starts no other job of `job_mib` now, or None when it starts one.
pub fn refusal(load: &HostLoad, job_mib: f64) -> Option<String> {
    let target = default_mib("offload_job_cpu_psi");
    if load.cpu_some_percent >= target {
        return Some(format!("CPU pressure {:.0}% at the {target:.0}% job target", load.cpu_some_percent));
    }
    memory_short(load.memory_total_mib, load.memory_available_mib, job_mib).then(|| format!("{:.0} MiB memory available, a job peaks at {job_mib:.0} MiB above the host's floor", load.memory_available_mib))
}
