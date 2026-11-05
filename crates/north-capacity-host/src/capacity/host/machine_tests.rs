// KEEP(bug): host cores come from the online CPU list, not the caller's cgroup quota; regression of 417cb70da (CPUQuota=0%)
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

// KEEP(bug): agent sessions and their desktop helpers in the agent slice never count as unleased agent pressure; regression of e5aea7dd0 (north#473)
#[test]
fn agent_sessions_and_their_desktop_helpers_in_agent_slice_never_count_as_unleased_agent_pressure() {
    use super::machine::unleased_agent_work;
    for unit in ["north-session-7c61d5a5-81a7-46fc-b06d-cb29db843209.scope", "north-fleet-gex45-disk-94360c08.service", "private-desktop-client-4242.scope", "agent-capacity.slice", "agent-capacity-fa5921172a774f2bb8ff97d89cbdd4df.scope"] {
        assert!(!unleased_agent_work(unit, "agent-capacity.slice"), "{unit}");
    }
    assert!(unleased_agent_work("agent-bounded.slice", "agent-capacity.slice"), "bounded jobs run outside any lease and still count");
}
