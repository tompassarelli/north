use crate::capacity::admission::{Class, Facts, Profile, Reason, decide};
use crate::capacity::fixtures::facts;
use crate::capacity::native_hold::LeaseFact;

fn heavy(started_seconds_ago: f64) -> LeaseFact {
    LeaseFact { id: format!("heavy-{started_seconds_ago}"), class: "heavy".to_owned(), owner: "lane".to_owned(), quiet: false, pair: None, created_at: 100_000.0 - started_seconds_ago * 1000.0, memory_mib: 0.0, cpus: 6.0, landing: false, quiet_seconds: None, usage: None, new_clients: 0, running_clients: 0, clients_seen: 0, started_owners: Default::default() }
}

fn gex45(build_host: bool, cpu_some_percent: u32) -> Facts {
    Facts {
        profile: Profile::Unattended,
        cores: 20,
        build_host,
        memory_total_mib: 128_000.0,
        memory_available_mib: 100_000.0,
        now: 100_000.0,
        cpu_some_avg10_basis_points: cpu_some_percent * 100,
        host_cpu_some_avg10_basis_points: Some(cpu_some_percent * 100),
        cpu_pressure_measured: true,
        requested_cpus: 6.0,
        leased_batch_cpus: 24.0,
        peer_batch_runs: 4,
        leases: vec![heavy(5.0), heavy(60.0), heavy(90.0), heavy(120.0)],
        ..facts(Class::Heavy)
    }
}

#[test]
fn an_offloaded_heavy_run_waits_only_while_its_host_is_over_the_pressure_target() {
    assert_ne!(decide(&gex45(false, 10)), Reason::Run, "a host no other host builds on keeps its settle window and declared-CPU bound");
    assert_eq!(decide(&gex45(true, 10)), Reason::Run, "gex45 at 10% starts the run though a build started 5 s ago and the running charges pass its batch CPUs");
    assert_ne!(decide(&gex45(true, 40)), Reason::Run, "at the 40% target the run waits");
}
