use super::admission::*;

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
        "newClients": new_clients, "gpuRuns": 0, "nativeWaiting": 0, "besideNative": false,
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

mod properties {
    use super::*;
    use proptest::prelude::*;

    fn profile() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("attended"), Just("unattended")]
    }

    fn class() -> impl Strategy<Value = Class> {
        prop_oneof![Just(Class::Moderate), Just(Class::Native), Just(Class::Heavy), Just(Class::Gpu), Just(Class::Exclusive)]
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

        #[test]
        fn prop_adding_load_never_turns_a_deferral_into_a_run(
            profile in profile(),
            class in class(),
            cores in 1u32..=64,
            memory_total_mib in 4096.0..262_144.0f64,
            before in (0.0..64.0f64, 0.0..64.0f64, 0.0..1.0f64, 0.0..1.0f64, 0u32..6000, 0u32..3000),
            more in (0.0..64.0f64, 0.0..64.0f64, 0.0..1.0f64, 0.0..1.0f64, 0u32..6000, 0u32..3000),
            requested_cpus in 0.0..32.0f64,
            requested_share in 0.001..0.5f64,
        ) {
            let mut facts = facts(profile, class, cores, memory_total_mib);
            (facts.requested_cpus, facts.requested_memory_mib) = (requested_cpus, memory_total_mib * requested_share);
            let load = |facts: &Facts, (batch, native, leased_memory, used_memory, pressure, protected): (f64, f64, f64, f64, u32, u32)| {
                let mut loaded = facts.clone();
                loaded.leased_batch_cpus += batch;
                loaded.leased_native_cpus += native;
                loaded.leased_memory_mib += memory_total_mib * leased_memory;
                loaded.memory_available_mib = (loaded.memory_available_mib * (1.0 - used_memory)).max(1.0);
                loaded.cpu_some_avg10_basis_points += pressure;
                loaded.protected_cpu_some_avg10_basis_points += protected;
                loaded
            };
            let light = load(&facts, before);
            let heavier = load(&light, more);
            let (was, now) = (decide(&light), decide(&heavier));
            prop_assert!(was == Reason::Run || now != Reason::Run, "{was:?} became {now:?} under more load: {heavier:?}");
        }

        #[test]
        fn prop_warcraft_client_cap_never_exceeds_its_ceiling(
            profile in profile(),
            host_warcraft_clients in proptest::option::of(0u32..32),
            gpu_clients in 0u32..32,
            bar1_free_mib in proptest::option::of(0.0..65_536.0f64),
            vram_free_mib in proptest::option::of(0.0..65_536.0f64),
            host_bar1_margin_mib in proptest::option::of(0u32..4096),
        ) {
            let mut facts = native_facts(profile, host_warcraft_clients, gpu_clients, 0);
            (facts.bar1_free_mib, facts.vram_free_mib, facts.host_bar1_margin_mib) = (bar1_free_mib, vram_free_mib, host_bar1_margin_mib);
            let ceiling = host_warcraft_clients.or(attended(profile).then_some(4));
            let cap = warcraft_client_cap(&facts);
            if let Some(ceiling) = ceiling {
                prop_assert!(cap.is_some_and(|cap| cap <= ceiling), "cap {cap:?} over ceiling {ceiling}");
            }
            if ceiling.is_none() && bar1_free_mib.is_none() && vram_free_mib.is_none() {
                prop_assert_eq!(cap, None);
            }
        }
    }
}
