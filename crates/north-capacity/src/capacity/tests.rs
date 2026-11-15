#![cfg(test)]

use super::admission::{Class, Facts, Profile, Reason, decide};
use super::native_hold::LeaseFact;

pub(super) use super::fixtures::facts;

pub(super) fn running(class: &str) -> LeaseFact {
    LeaseFact { id: format!("{class}-lease"), class: class.to_owned(), owner: "lane".to_owned(), quiet: false, pair: None, created_at: 0.0, memory_mib: 0.0, cpus: 0.0, landing: false, canary: false, quiet_seconds: None, usage: None, new_clients: 0, running_clients: 0, clients_seen: 0, started_owners: Default::default() }
}

#[test]
fn a_third_lease_on_an_eight_core_host_queues_when_two_already_charge_the_cores_it_would_pass_but_a_canary_check_takes_its_reserve() {
    let ccx33 = |class, landing: bool, count: usize| {
        let lease = LeaseFact { cpus: 3.0, landing, created_at: -1.0e9, ..running("heavy") };
        Facts {
            profile: Profile::Unattended,
            cores: 8,
            memory_total_mib: 32768.0,
            memory_available_mib: 28000.0,
            requested_cpus: 3.0,
            leased_batch_cpus: 3.0 * count as f64,
            peer_batch_runs: u32::try_from(count).unwrap(),
            leases: vec![lease; count],
            ..facts(class)
        }
    };
    for landing in [false, true] {
        let class = if landing { Class::Critical } else { Class::Heavy };
        assert_eq!(decide(&ccx33(class, landing, 0)), Reason::Run, "a lone request runs, landing {landing}");
        assert_eq!(decide(&ccx33(class, landing, 1)), Reason::Run, "a second lease fits beside one, landing {landing}");
        assert_eq!(decide(&ccx33(class, landing, 2)), Reason::DeferCpuCapacity, "the third passes 8 cores, landing {landing}");
    }
    let build_host = |count| Facts { build_host: true, host_cpu_some_avg10_basis_points: Some(5000), ..ccx33(Class::Critical, true, count) };
    assert_eq!(decide(&build_host(2)), Reason::DeferCpuCapacity, "on 2026-11-12 a build host under CPU stall ran seven land checks on 20 threads at load 50 to 70");
    assert_eq!(decide(&build_host(1)), Reason::Run, "the queued check starts once one ends");
    let canary = Facts { canary: true, requested_cpus: 2.0, ..ccx33(Class::Critical, true, 3) };
    assert_eq!(decide(&canary), Reason::Run, "on 2026-11-13 a canary's 2-CPU check waited 120 s on gex45 behind land checks charging 29 of its 20 CPUs");
    let mut beside_canary = canary;
    beside_canary.leases.push(LeaseFact { canary: true, ..beside_canary.leases[0].clone() });
    assert_eq!(decide(&beside_canary), Reason::DeferCpuCapacity, "a second canary check waits for the reserve");
}
