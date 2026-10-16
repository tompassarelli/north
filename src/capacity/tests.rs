use super::admission::{Class, Facts, Profile, Reason, decide};

fn facts(class: Class) -> Facts {
    Facts {
        profile: Profile::Attended,
        class,
        cores: 24,
        memory_total_mib: 96343.0,
        memory_available_mib: 70000.0,
        cpu_some_avg10_basis_points: 0,
        protected_cpu_some_avg10_basis_points: 0,
        leased_memory_mib: 0.0,
        leased_batch_cpus: 0.0,
        unleased_heavy_cores: 0.0,
        leased_native_cpus: 0.0,
        requested_cpus: 2.0,
        requested_memory_mib: 2048.0,
        peer_batch_runs: 0,
        peer_exclusive_runs: 0,
        unbounded_runs: 0,
        queued_ahead: 0,
        exclusive_waiting: 0,
        gpu_busy_percent: 0,
        gpu_clients: 0,
        new_clients: 0,
        gpu_runs: 0,
        native_waiting: 0,
        quiet_native_runs: 0,
        beside_native: false,
    }
}

fn beside(class: Class, quiet_native_runs: u32, beside_native: bool) -> Reason {
    decide(&Facts { quiet_native_runs, beside_native, ..facts(class) })
}

#[test]
fn live_quiet_native_session_holds_batch_work_until_it_ends() {
    for class in [Class::Moderate, Class::Heavy, Class::Gpu, Class::Exclusive] {
        assert_eq!(beside(class, 1, false), Reason::DeferNativeQuiet, "{class:?}");
        assert_eq!(beside(class, 0, false), Reason::Run, "{class:?}");
    }
}

#[test]
fn light_native_landing_and_tom_leases_run_beside_native() {
    for class in [Class::Agent, Class::Native, Class::Critical, Class::Tom] {
        assert_eq!(beside(class, 2, false), Reason::Run, "{class:?}");
    }
}

#[test]
fn native_owner_steps_opt_out_explicitly() {
    assert_eq!(beside(Class::Moderate, 1, true), Reason::Run);
    assert_eq!(beside(Class::Gpu, 1, true), Reason::Run);
}

#[test]
fn four_core_history_request_admits_only_at_its_clamped_grant() {
    let host = Facts {
        profile: Profile::Unattended,
        cores: 4,
        memory_total_mib: 16384.0,
        memory_available_mib: 14000.0,
        requested_memory_mib: 2429.0,
        ..facts(Class::Heavy)
    };
    assert_eq!(decide(&Facts { requested_cpus: 5.0, ..host.clone() }), Reason::DeferCpuCapacity);
    assert_eq!(decide(&Facts { requested_cpus: 4.0, ..host }), Reason::Run);
}

#[test]
fn unleased_heavy_cpu_reduces_admitted_capacity() {
    let mut host = facts(Class::Moderate);
    host.leased_batch_cpus = 16.0;
    host.unleased_heavy_cores = 3.0;
    assert_eq!(decide(&host), Reason::DeferCpuCapacity);
    host.unleased_heavy_cores = 2.0;
    assert_eq!(decide(&host), Reason::Run);
}
