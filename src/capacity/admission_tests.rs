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
