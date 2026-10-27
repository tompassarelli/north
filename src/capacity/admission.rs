use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

const INTERACTIVE_PRESSURE_BASIS_POINTS: u32 = 1000;
const BATCH_PRESSURE_BASIS_POINTS: u32 = 3000;
const GPU_BUSY_PERCENT: u32 = 85;
const ATTENDED_GPU_CLIENTS: u32 = 4;
const GPU_SLOTS: u32 = 2;

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| {
    include_str!("../../harness/defaults.toml")
        .parse()
        .expect("harness/defaults.toml parses")
});

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
    #[serde(default, rename = "vramFreeMiB")]
    pub(crate) vram_free_mib: Option<f64>,
    #[serde(default, rename = "hostBar1MarginMiB")]
    pub(crate) host_bar1_margin_mib: Option<u32>,
    pub(crate) new_clients: u32,
    #[serde(default)]
    pub(crate) new_desktops: u32,
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
    pub(crate) queued_ahead_new_clients: Vec<u32>,
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
    #[serde(rename = "bar1NeedMiB")]
    pub(crate) bar1_need_mib: f64,
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
        .then(|| facts.bar1_free_mib.map(|bar1_free_mib| Aperture { bar1_free_mib, bar1_need_mib: bar1_need_mib(facts) }))
        .flatten();
    Admission { reason, offload, aperture }
}

pub(crate) fn starts_gpu_work(facts: &Facts) -> bool {
    match facts.class {
        Class::Native => facts.new_clients > 0 || facts.new_desktops > 0,
        Class::Gpu => true,
        _ => false,
    }
}

fn bar1_need_mib(facts: &Facts) -> f64 {
    let clients = match facts.class {
        Class::Gpu => 1,
        _ => facts.new_clients,
    };
    aperture_need_mib(facts.new_desktops, clients, facts.host_bar1_margin_mib)
}

pub(crate) fn aperture_need_mib(desktops: u32, clients: u32, host_margin_mib: Option<u32>) -> f64 {
    let margin = host_margin_mib.map_or_else(|| default_mib("bar1_margin_mib"), f64::from);
    f64::from(desktops) * default_mib("bar1_desktop_mib") + f64::from(clients) * default_mib("bar1_client_mib") + margin
}

pub(super) fn default_mib(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value
        .as_float()
        .or_else(|| value.as_integer().map(|whole| whole as f64))
        .unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

fn aperture_short(facts: &Facts) -> bool {
    starts_gpu_work(facts) && facts.bar1_free_mib.is_some_and(|free| free < bar1_need_mib(facts))
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

/// Whether an unattended host of `total` MiB, idle, admits a request of `requested` MiB.
pub(crate) fn memory_admits(total: f64, requested: f64) -> bool {
    !memory_short(total, total, requested) && requested * 4.0 <= total * 3.0
}

/// The MiB a dispatched box of `class` asks of its host: the heavy request for a test-verified box,
/// else the agent one.
pub(crate) fn dispatch_request_mib(class: Class) -> f64 {
    match class {
        Class::Heavy => default_mib("dispatch_heavy_memory_mib"),
        _ => default_mib("dispatch_agent_memory_mib"),
    }
}

pub(crate) fn offload_room() -> (f64, f64) {
    (default_mib("land_offload_cpus"), default_mib("land_offload_disk_free_gib"))
}

/// Whether unattended memory headroom defers a request of `requested` MiB now.
pub(crate) fn memory_short(total: f64, available: f64, requested: f64) -> bool {
    available - requested < memory_floor_mib(Profile::Unattended, total)
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

/// The Warcraft clients the host takes: its static count as a ceiling, lowered to the clients it
/// runs plus those its measured free BAR1 and VRAM still fit.
pub(crate) fn warcraft_client_cap(facts: &Facts) -> Option<u32> {
    let ceiling = facts.host_warcraft_clients.or((facts.profile == Profile::Attended).then_some(ATTENDED_GPU_CLIENTS));
    let fits = |free: Option<f64>, reserve: f64, each: f64| free.map(|free| ((free - reserve) / each).floor().max(0.0) as u32);
    let bar1_margin = facts.host_bar1_margin_mib.map_or_else(|| default_mib("bar1_margin_mib"), f64::from);
    let bar1 = fits(facts.bar1_free_mib, bar1_margin, default_mib("bar1_client_mib"));
    let vram = fits(facts.vram_free_mib, default_mib("vram_margin_mib"), default_mib("vram_client_mib"));
    let measured = bar1.into_iter().chain(vram).min().map(|more| facts.gpu_clients.saturating_add(more));
    match (ceiling, measured) {
        (Some(ceiling), Some(measured)) => Some(ceiling.min(measured)),
        (ceiling, measured) => ceiling.or(measured),
    }
}

fn over_client_cap(facts: &Facts, new_clients: u32) -> bool {
    warcraft_client_cap(facts).is_some_and(|cap| facts.gpu_clients.saturating_add(new_clients) > cap)
}

/// Native waiters ahead in line, less those whose own clients would defer as DEFER_GPU_CLIENTS: they hold no place, so a smaller start behind them takes a slot that frees.
fn native_queued_ahead(facts: &Facts) -> u32 {
    if facts.queued_ahead_new_clients.is_empty() {
        return facts.queued_ahead;
    }
    let holding = facts.queued_ahead_new_clients.iter().filter(|new_clients| !over_client_cap(facts, **new_clients)).count();
    u32::try_from(holding).unwrap_or(u32::MAX)
}

fn native(facts: &Facts) -> Reason {
    if (facts.new_clients > 0 || facts.new_desktops > 0) && native_queued_ahead(facts) > 0 {
        return Reason::DeferQueued;
    }
    if facts.quiet && facts.peer_exclusive_runs > u32::from(facts.within_exclusive) {
        return Reason::DeferExclusive;
    }
    if facts.leased_native_cpus + facts.requested_cpus > aggregate_cpus(facts.profile, facts.cores) {
        return Reason::DeferNativeCpus;
    }
    if over_client_cap(facts, facts.new_clients) {
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
        decide(&native_facts(profile, host_warcraft_clients, gpu_clients, new_clients))
    }

    fn native_facts(profile: &str, host_warcraft_clients: Option<u32>, gpu_clients: u32, new_clients: u32) -> Facts {
        serde_json::from_value(serde_json::json!({
            "profile": profile, "hostWarcraftClients": host_warcraft_clients, "class": "native", "cores": 24, "memoryTotalMiB": 96343.0, "memoryAvailableMiB": 80000.0,
            "cpuSomeAvg10BasisPoints": 0, "protectedCpuSomeAvg10BasisPoints": 0, "leasedMemoryMiB": 0.0, "leasedBatchCpus": 0.0,
            "leasedNativeCpus": 0.0, "requestedCpus": 2.0, "requestedMemoryMiB": 4096.0, "peerBatchRuns": 0, "peerExclusiveRuns": 0,
            "unboundedRuns": 0, "queuedAhead": 0, "exclusiveWaiting": 0, "gpuBusyPercent": 0, "gpuClients": gpu_clients,
            "newClients": new_clients, "gpuRuns": 0, "nativeWaiting": 0, "quietNativeRuns": 0, "besideNative": false,
        }))
        .unwrap()
    }

    #[test]
    fn attended_client_cap_counts_only_the_clients_a_request_starts() {
        assert_eq!(native_with(4, 0), Reason::Run);
        assert_eq!(native_with(3, 2), Reason::DeferGpuClients);
        assert_eq!(native_with(2, 2), Reason::Run);
    }

    #[test]
    fn a_queued_native_start_that_cannot_fit_holds_no_place_for_a_smaller_one_behind_it() {
        let behind = |gpu_clients, queued_ahead_new_clients: &[u32]| {
            let mut facts = native_facts("attended", None, gpu_clients, 1);
            facts.queued_ahead = u32::try_from(queued_ahead_new_clients.len()).unwrap();
            facts.queued_ahead_new_clients = queued_ahead_new_clients.to_vec();
            decide(&facts)
        };
        assert_eq!(behind(3, &[2]), Reason::Run);
        assert_eq!(behind(3, &[1]), Reason::DeferQueued);
        assert_eq!(behind(2, &[2]), Reason::DeferQueued);
        assert_eq!(behind(3, &[2, 1]), Reason::DeferQueued);
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
