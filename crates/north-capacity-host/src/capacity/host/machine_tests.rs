use serde_json::json;

use super::machine::latency_scopes;
use super::systemd::scope_unit;

#[test]
fn only_live_native_leases_that_declared_latency_count_as_pressure_on_batch_work() {
    let leases = [
        json!({ "id": "aaaa-1", "class": "native", "owner": "wisp-online-clone-b", "latency": true }),
        json!({ "id": "bbbb-2", "class": "native", "owner": "wisp-lan-pair-4" }),
        json!({ "id": "cccc-3", "class": "native", "owner": "capture-batch", "quiet": true }),
        json!({ "id": "dddd-4", "class": "heavy", "owner": "build", "latency": true }),
    ];
    assert_eq!(latency_scopes(&leases), vec![scope_unit("aaaa-1")]);
    assert!(latency_scopes(&leases[1..3]).is_empty(), "capture, accept and pool clients never hold batch work through pressure");
}

#[test]
fn agent_sessions_and_their_desktop_helpers_in_agent_slice_never_count_as_unleased_agent_pressure() {
    use super::machine::unleased_agent_work;
    for unit in ["north-session-7c61d5a5-81a7-46fc-b06d-cb29db843209.scope", "north-fleet-gex45-disk-94360c08.service", "private-desktop-client-4242.scope", "agent-capacity.slice", "agent-capacity-fa5921172a774f2bb8ff97d89cbdd4df.scope"] {
        assert!(!unleased_agent_work(unit, "agent-capacity.slice"), "{unit}");
    }
    assert!(unleased_agent_work("agent-bounded.slice", "agent-capacity.slice"), "bounded jobs run outside any lease and still count");
}

#[test]
fn host_cores_counts_the_online_cpu_list_and_ignores_the_callers_cgroup_quota() {
    use super::machine::online_cpus;
    assert_eq!(online_cpus(Some("0-19\n")), 20.0);
    assert_eq!(online_cpus(Some("0,2-3,8-11")), 7.0);
    assert_eq!(online_cpus(Some("0")), 1.0);
    let fallback = std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64);
    for unusable in [None, Some(""), Some("garbage"), Some("5-2")] {
        assert_eq!(online_cpus(unusable), fallback, "{unusable:?}");
    }
}
