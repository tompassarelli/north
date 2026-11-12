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

#[test]
fn a_land_check_waits_out_of_its_offload_slot_while_the_land_checks_leased_or_queued_fill_the_cores_and_a_lone_check_always_starts() {
    let root = tempfile::tempdir().unwrap();
    let write = |directory: &str, name: &str, record: serde_json::Value| {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
        std::fs::write(root.path().join(directory).join(name), record.to_string()).unwrap();
    };
    let refusal = || super::landing_refusal_under(root.path(), 8.0, 20.0);
    assert_eq!(refusal(), None, "a lone land check always starts");
    write("leases", "worker.json", serde_json::json!({"landing": false, "cpus": 16.0}));
    write("queue", "worker.json", serde_json::json!({"class": "heavy", "requestedCpus": 16.0}));
    write("leases", "a.json", serde_json::json!({"landing": true, "cpus": 4.0}));
    write("queue", "b.json", serde_json::json!({"class": "critical", "requestedCpus": 4.0}));
    assert_eq!(refusal(), None, "two checks charged 4 leave room for one charged 8 on 20 cores; other work counts not");
    write("leases", "a.json", serde_json::json!({"landing": true, "cpus": 8.0}));
    write("queue", "b.json", serde_json::json!({"class": "critical", "requestedCpus": 8.0}));
    assert_eq!(refusal().as_deref(), Some("land checks leased or queued here charge 16 of its 20 cores, so another's 8 waits for one to end"), "gex45 on 2026-11-12: two checks charged 8 leave no room for a third");
}
