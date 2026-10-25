use serde::{Deserialize, Serialize};

const INTERACTIVE_PRESSURE_BASIS_POINTS: u32 = 1000;
const BATCH_PRESSURE_BASIS_POINTS: u32 = 3000;
const GPU_BUSY_PERCENT: u32 = 85;
const ATTENDED_GPU_CLIENTS: u32 = 4;
const GPU_SLOTS: u32 = 2;
const BAR1_RESERVE_MIB: u32 = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Profile {
    Attended,
    Unattended,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Class {
    Agent,
    Native,
    Moderate,
    Heavy,
    Exclusive,
    Gpu,
    Critical,
    Tom,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Facts {
    pub(crate) profile: Profile,
    pub(crate) class: Class,
    pub(crate) cores: u32,
    #[serde(rename = "memoryTotalMiB")]
    pub(crate) memory_total_mib: f64,
    #[serde(rename = "memoryAvailableMiB")]
    pub(crate) memory_available_mib: f64,
    pub(crate) cpu_some_avg10_basis_points: u32,
    pub(crate) protected_cpu_some_avg10_basis_points: u32,
    #[serde(rename = "leasedMemoryMiB")]
    pub(crate) leased_memory_mib: f64,
    pub(crate) leased_batch_cpus: f64,
    #[serde(default)]
    pub(crate) unleased_heavy_cores: f64,
    pub(crate) leased_native_cpus: f64,
    pub(crate) requested_cpus: f64,
    #[serde(rename = "requestedMemoryMiB")]
    pub(crate) requested_memory_mib: f64,
    pub(crate) peer_batch_runs: u32,
    pub(crate) peer_exclusive_runs: u32,
    pub(crate) unbounded_runs: u32,
    pub(crate) queued_ahead: u32,
    pub(crate) exclusive_waiting: u32,
    #[serde(default)]
    pub(crate) before_drain: bool,
    pub(crate) gpu_busy_percent: u32,
    pub(crate) gpu_clients: u32,
    #[serde(default)]
    pub(crate) host_warcraft_clients: Option<u32>,
    #[serde(default, rename = "bar1FreeMiB")]
    pub(crate) bar1_free_mib: Option<f64>,
    #[serde(default, rename = "bar1TimedOut")]
    pub(crate) bar1_timed_out: bool,
    #[serde(default, rename = "hostBar1ReserveMiB")]
    pub(crate) host_bar1_reserve_mib: Option<u32>,
    pub(crate) new_clients: u32,
    pub(crate) gpu_runs: u32,
    pub(crate) native_waiting: u32,
    pub(crate) quiet_native_runs: u32,
    pub(crate) beside_native: bool,
    #[serde(default)]
    pub(crate) quiet: bool,
    #[serde(default)]
    pub(crate) exclusive_leases: Vec<String>,
    #[serde(skip)]
    pub(crate) within_exclusive: bool,
    #[serde(default)]
    pub(crate) gaming: bool,
    #[serde(default)]
    pub(crate) gaming_class: Option<Class>,
    #[serde(default)]
    pub(crate) queued_ahead_classes: Vec<Option<Class>>,
    #[serde(default)]
    pub(crate) run_queue_load: Option<f64>,
    #[serde(default)]
    pub(crate) need: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Admission {
    pub(crate) reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) offload: Option<String>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub(crate) aperture: Option<Aperture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct Aperture {
    #[serde(rename = "bar1FreeMiB")]
    pub(crate) bar1_free_mib: f64,
    #[serde(rename = "bar1ReserveMiB")]
    pub(crate) bar1_reserve_mib: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Reason {
    Run,
    Invalid,
    DeferMemoryHeadroom,
    DeferMemoryCapacity,
    DeferNativeCpus,
    DeferGpuClients,
    DeferGpuBusy,
    DeferGpuAperture,
    DeferGpuApertureTimeout,
    DeferUnboundedPeer,
    DeferExclusive,
    DeferInteractivePressure,
    DeferNativeQuiet,
    DeferNativeWaiting,
    DeferGpuSlots,
    DeferQueued,
    DeferExclusiveQueued,
    DeferCpuCapacity,
    DeferCpuPressure,
    DeferGaming,
    DeferRunQueue,
}

pub(crate) fn admit(facts: &Facts, serving: impl Fn(&str) -> Option<String>) -> Admission {
    let reason = decide(facts);
    let offload = match (reason, facts.need.as_deref()) {
        (Reason::DeferRunQueue, Some(needs)) => serving(needs),
        _ => None,
    };
    let aperture = (reason == Reason::DeferGpuAperture)
        .then(|| facts.bar1_free_mib.map(|bar1_free_mib| Aperture { bar1_free_mib, bar1_reserve_mib: bar1_reserve_mib(facts) }))
        .flatten();
    Admission { reason, offload, aperture }
}

pub(crate) fn starts_gpu_work(facts: &Facts) -> bool {
    match facts.class {
        Class::Native => facts.new_clients > 0,
        Class::Gpu => true,
        _ => false,
    }
}

fn bar1_reserve_mib(facts: &Facts) -> u32 {
    facts.host_bar1_reserve_mib.unwrap_or(BAR1_RESERVE_MIB)
}

fn aperture_short(facts: &Facts) -> bool {
    starts_gpu_work(facts) && facts.bar1_free_mib.is_some_and(|free| free < f64::from(bar1_reserve_mib(facts)))
}

fn run_queue_overloaded(facts: &Facts) -> bool {
    matches!(facts.class, Class::Heavy | Class::Exclusive) && facts.run_queue_load.is_some_and(|load| load > f64::from(facts.cores))
}

pub(crate) fn decide(facts: &Facts) -> Reason {
    if invalid(facts) {
        return Reason::Invalid;
    }
    if facts.gaming && gaming_deferred(facts.gaming_class.unwrap_or(facts.class)) {
        return Reason::DeferGaming;
    }
    if let Some(reason) = memory(facts) {
        return reason;
    }
    if run_queue_overloaded(facts) {
        return Reason::DeferRunQueue;
    }
    if starts_gpu_work(facts) && facts.bar1_timed_out {
        return Reason::DeferGpuApertureTimeout;
    }
    if aperture_short(facts) {
        return Reason::DeferGpuAperture;
    }
    match facts.class {
        Class::Agent => Reason::Run,
        Class::Native => native(facts),
        _ => batch(facts),
    }
}

fn gaming_deferred(class: Class) -> bool {
    matches!(class, Class::Native | Class::Heavy | Class::Exclusive | Class::Gpu)
}

fn queued_ahead(facts: &Facts) -> u32 {
    if !facts.gaming || gaming_deferred(facts.gaming_class.unwrap_or(facts.class)) {
        return facts.queued_ahead;
    }
    let held = facts.queued_ahead_classes.iter().flatten().filter(|class| gaming_deferred(**class)).count();
    facts.queued_ahead.saturating_sub(u32::try_from(held).unwrap_or(u32::MAX))
}

fn invalid(facts: &Facts) -> bool {
    facts.cores == 0
        || facts.memory_total_mib <= 0.0
        || facts.memory_available_mib <= 0.0
        || facts.requested_cpus < 0.0
        || facts.requested_memory_mib <= 0.0
}

fn memory(facts: &Facts) -> Option<Reason> {
    if facts.memory_available_mib - facts.requested_memory_mib < memory_floor_mib(facts.profile, facts.memory_total_mib) {
        return Some(Reason::DeferMemoryHeadroom);
    }
    ((facts.leased_memory_mib + facts.requested_memory_mib) * 4.0 > facts.memory_total_mib * 3.0)
        .then_some(Reason::DeferMemoryCapacity)
}

fn memory_floor_mib(profile: Profile, total: f64) -> f64 {
    let share = match profile {
        Profile::Attended => total / 5.0,
        Profile::Unattended => total / 12.0,
    };
    if total < 32768.0 { share } else { share.max(8192.0).min(total / 4.0) }
}

fn aggregate_cpus(profile: Profile, cores: u32) -> f64 {
    let reserved = match profile {
        Profile::Attended if cores > 8 => 4,
        Profile::Attended if cores > 4 => 2,
        Profile::Attended => 1,
        Profile::Unattended => 0,
    };
    f64::from(cores - reserved)
}

fn warcraft_client_cap(facts: &Facts) -> Option<u32> {
    facts.host_warcraft_clients.or((facts.profile == Profile::Attended).then_some(ATTENDED_GPU_CLIENTS))
}

fn native(facts: &Facts) -> Reason {
    if facts.quiet && facts.peer_exclusive_runs > u32::from(facts.within_exclusive) {
        return Reason::DeferExclusive;
    }
    if facts.leased_native_cpus + facts.requested_cpus > aggregate_cpus(facts.profile, facts.cores) {
        return Reason::DeferNativeCpus;
    }
    if warcraft_client_cap(facts).is_some_and(|cap| facts.gpu_clients.saturating_add(facts.new_clients) > cap) {
        return Reason::DeferGpuClients;
    }
    if facts.gpu_busy_percent >= GPU_BUSY_PERCENT && facts.gpu_runs == 0 {
        return Reason::DeferGpuBusy;
    }
    Reason::Run
}

fn interactive_pressure(facts: &Facts) -> bool {
    facts.profile == Profile::Attended && facts.protected_cpu_some_avg10_basis_points >= INTERACTIVE_PRESSURE_BASIS_POINTS
}

fn batch(facts: &Facts) -> Reason {
    if facts.unbounded_runs > 0 {
        return Reason::DeferUnboundedPeer;
    }
    if facts.peer_exclusive_runs > 0 {
        return Reason::DeferExclusive;
    }
    if matches!(facts.class, Class::Critical | Class::Tom) {
        return if interactive_pressure(facts) { Reason::DeferInteractivePressure } else { Reason::Run };
    }
    if facts.quiet_native_runs > 0 && !facts.beside_native {
        return Reason::DeferNativeQuiet;
    }
    if facts.class == Class::Gpu {
        if facts.native_waiting > 0 {
            return Reason::DeferNativeWaiting;
        }
        if facts.gpu_runs >= GPU_SLOTS {
            return Reason::DeferGpuSlots;
        }
    }
    if facts.class == Class::Exclusive {
        return if facts.exclusive_waiting > 0 {
            Reason::DeferQueued
        } else if facts.peer_batch_runs > 0 {
            Reason::DeferExclusive
        } else {
            Reason::Run
        };
    }
    shared(facts)
}

fn shared(facts: &Facts) -> Reason {
    if facts.exclusive_waiting > 0 && facts.class == Class::Heavy && !facts.before_drain {
        return Reason::DeferExclusiveQueued;
    }
    if interactive_pressure(facts) {
        return Reason::DeferInteractivePressure;
    }
    if queued_ahead(facts) > 0 {
        return Reason::DeferQueued;
    }
    if facts.leased_batch_cpus + facts.unleased_heavy_cores + facts.requested_cpus > aggregate_cpus(facts.profile, facts.cores) {
        return Reason::DeferCpuCapacity;
    }
    if facts.cpu_some_avg10_basis_points > BATCH_PRESSURE_BASIS_POINTS {
        return Reason::DeferCpuPressure;
    }
    Reason::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_with(gpu_clients: u32, new_clients: u32) -> Reason {
        native_on("attended", None, gpu_clients, new_clients)
    }

    fn native_on(profile: &str, host_warcraft_clients: Option<u32>, gpu_clients: u32, new_clients: u32) -> Reason {
        let facts: Facts = serde_json::from_value(serde_json::json!({
            "profile": profile, "hostWarcraftClients": host_warcraft_clients, "class": "native", "cores": 24, "memoryTotalMiB": 96343.0, "memoryAvailableMiB": 80000.0,
            "cpuSomeAvg10BasisPoints": 0, "protectedCpuSomeAvg10BasisPoints": 0, "leasedMemoryMiB": 0.0, "leasedBatchCpus": 0.0,
            "leasedNativeCpus": 0.0, "requestedCpus": 2.0, "requestedMemoryMiB": 4096.0, "peerBatchRuns": 0, "peerExclusiveRuns": 0,
            "unboundedRuns": 0, "queuedAhead": 0, "exclusiveWaiting": 0, "gpuBusyPercent": 0, "gpuClients": gpu_clients,
            "newClients": new_clients, "gpuRuns": 0, "nativeWaiting": 0, "quietNativeRuns": 0, "besideNative": false,
        }))
        .unwrap();
        decide(&facts)
    }

    #[test]
    fn attended_client_cap_counts_only_the_clients_a_request_starts() {
        assert_eq!(native_with(4, 0), Reason::Run);
        assert_eq!(native_with(3, 2), Reason::DeferGpuClients);
        assert_eq!(native_with(2, 2), Reason::Run);
    }

    #[test]
    fn a_host_measured_client_count_replaces_the_attended_four_in_either_profile() {
        for profile in ["attended", "unattended"] {
            assert_eq!(native_on(profile, Some(8), 6, 2), Reason::Run, "{profile}");
            assert_eq!(native_on(profile, Some(8), 7, 2), Reason::DeferGpuClients, "{profile}");
        }
        assert_eq!(native_on("unattended", None, 40, 2), Reason::Run);
    }
}
