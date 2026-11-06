#![cfg(test)]

use super::admission::*;

fn native_facts(profile: &str, host_warcraft_clients: Option<u32>, gpu_clients: u32, new_clients: u32) -> Facts {
    serde_json::from_value(serde_json::json!({
        "profile": profile, "hostWarcraftClients": host_warcraft_clients, "class": "native", "cores": 24, "memoryTotalMiB": 96343.0, "memoryAvailableMiB": 80000.0,
        "cpuSomeAvg10BasisPoints": 0, "protectedCpuSomeAvg10BasisPoints": 0, "leasedMemoryMiB": 0.0, "leasedBatchCpus": 0.0,
        "leasedNativeCpus": 0.0, "requestedCpus": 2.0, "requestedMemoryMiB": 4096.0, "peerBatchRuns": 0, "peerExclusiveRuns": 0,
        "unboundedRuns": 0, "queuedAhead": 0, "exclusiveWaiting": 0, "gpuBusyPercent": 0, "gpuClients": gpu_clients,
        "newClients": new_clients, "gpuRuns": 0, "nativeWaiting": 0, "besideNative": false,
    }))
    .unwrap()
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    fn profile() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("attended"), Just("unattended")]
    }

    fn attended(profile: &str) -> bool {
        profile == "attended"
    }

    fn room_cpus(profile: &str, cores: u32) -> f64 {
        let reserved = match (attended(profile), cores) {
            (false, _) => 0,
            (true, cores) if cores > 8 => 4,
            (true, cores) if cores > 4 => 2,
            (true, _) => 1,
        };
        f64::from(cores.saturating_sub(reserved).max(1))
    }

    fn facts(profile: &str, class: Class, cores: u32, memory_total_mib: f64) -> Facts {
        let mut facts = native_facts(profile, None, 0, 0);
        (facts.class, facts.cores, facts.memory_total_mib, facts.memory_available_mib) = (class, cores, memory_total_mib, memory_total_mib);
        facts
    }

    proptest! {
        // KEEP(mutant): a moderate or native request under every limit is never deferred for room, on any profile and core count; owns the room arithmetic of decide
        #[test]
        fn prop_a_moderate_or_native_request_under_every_limit_is_never_deferred_for_room(
            profile in profile(),
            native in any::<bool>(),
            cores in 1u32..=64,
            memory_total_mib in 4096.0..262_144.0f64,
            available_share in 0.3..1.0f64,
            shares in (0.0..0.99f64, 0.0..0.99f64, 0.01..0.99f64, 0.0..0.99f64),
        ) {
            let (request_share, leased_share, memory_share, leased_memory_share) = shares;
            let class = if native { Class::Native } else { Class::Moderate };
            let mut facts = facts(profile, class, cores, memory_total_mib);
            let room = room_cpus(profile, cores);
            facts.requested_cpus = room * request_share;
            let leased = (room - facts.requested_cpus) * leased_share;
            if native { facts.leased_native_cpus = leased } else { facts.leased_batch_cpus = leased }
            facts.memory_available_mib = memory_total_mib * available_share;
            let headroom = facts.memory_available_mib - memory_floor_mib(facts.profile, memory_total_mib);
            prop_assume!(headroom > 0.0);
            facts.requested_memory_mib = headroom.min(memory_total_mib * 0.75) * memory_share;
            facts.leased_memory_mib = (memory_total_mib * 0.75 - facts.requested_memory_mib) * leased_memory_share;
            prop_assert_eq!(decide(&facts), Reason::Run, "{:?}", facts);
        }
    }
}

#[test]
fn a_waiting_land_check_runs_ahead_of_queued_worker_builds_which_it_refuses() {
    let gex45 = |class, land_check_waiting, queued_ahead| Facts { profile: Profile::Unattended, cores: 20, leased_batch_cpus: 2.0, requested_cpus: 6.0, queued_ahead, land_check_waiting, ..super::fixtures::facts(class) };
    assert_eq!(decide(&gex45(Class::Heavy, false, 0)), Reason::Run, "with no land check waiting the host admits a worker build");
    for class in [Class::Heavy, Class::Moderate, Class::Exclusive] {
        assert_eq!(decide(&gex45(class, true, 0)), Reason::DeferLandCheckWaiting, "{class:?} worker lease while a land check waits");
    }
    assert_eq!(decide(&gex45(Class::Heavy, true, 3)), Reason::DeferLandCheckWaiting, "a worker build queued first still yields");
    assert_eq!(decide(&gex45(Class::Critical, true, 3)), Reason::Run, "the land check runs as critical ahead of the queued worker builds");
    for class in [Class::Agent, Class::Gpu, Class::Tom, Class::Runtime] {
        assert_ne!(decide(&gex45(class, true, 0)), Reason::DeferLandCheckWaiting, "{class:?} keeps its own rules");
    }
    assert_eq!(decide(&Facts { holds_offload_slot: true, ..gex45(Class::Heavy, true, 0) }), Reason::Run, "a worker build already holding an offload slot runs, since yielding would keep that slot from the land check");
    assert_eq!(serde_json::to_value(Reason::DeferLandCheckWaiting).unwrap(), "DEFER_LAND_CHECK_WAITING");
    assert_eq!(super::land_claim::REASON, "land check waiting");
}
