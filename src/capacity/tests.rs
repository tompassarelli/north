use super::admission::{Admission, Class, Facts, Profile, Reason, admit, decide};

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
        before_drain: false,
        gpu_busy_percent: 0,
        gpu_clients: 0,
        new_clients: 0,
        gpu_runs: 0,
        native_waiting: 0,
        quiet_native_runs: 0,
        beside_native: false,
        gaming: false,
        gaming_class: None,
        queued_ahead_classes: Vec::new(),
        run_queue_load: None,
        need: None,
    }
}

const OVERLOAD: &str = include_str!("../../harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json");

fn overload(class: &str, need: Option<&str>) -> (Facts, toml_edit::DocumentMut) {
    let recorded: serde_json::Value = serde_json::from_str(OVERLOAD).unwrap();
    let mut facts = recorded["facts"].clone();
    facts["class"] = class.into();
    facts["need"] = need.into();
    (serde_json::from_value(facts).unwrap(), recorded["hosts"].as_str().unwrap().parse().unwrap())
}

fn overload_admission(class: &str, need: Option<&str>) -> Admission {
    let (facts, hosts) = overload(class, need);
    admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "here"))
}

#[test]
fn run_queue_over_thread_count_refuses_heavy_and_exclusive_naming_the_serving_host() {
    for class in ["heavy", "exclusive"] {
        let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned) };
        assert_eq!(overload_admission(class, Some("native:smashcraft")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:smashcraft")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:galileo")), refused(None), "{class}");
        assert_eq!(overload_admission(class, None), refused(None), "{class}");
    }
}

#[test]
fn run_queue_offload_names_only_another_host_that_serves_every_need() {
    let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned) };
    assert_eq!(overload_admission("heavy", Some("native:smashcraft,private:smashcraft")), refused(Some("gpu-host")));
    assert_eq!(overload_admission("heavy", Some("native:smashcraft,private:galileo")), refused(None));
    let (facts, hosts) = overload("heavy", Some("native:smashcraft"));
    assert_eq!(admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "gpu-host")), refused(None));
}

#[test]
fn run_queue_over_thread_count_still_admits_moderate_critical_and_tom() {
    for class in ["moderate", "critical", "tom"] {
        assert_eq!(overload_admission(class, Some("native:smashcraft")), Admission { reason: Reason::Run, offload: None }, "{class}");
    }
}

#[test]
fn run_queue_at_thread_count_still_admits_heavy() {
    let (mut facts, _) = overload("heavy", Some("native:smashcraft"));
    facts.run_queue_load = Some(f64::from(facts.cores));
    assert_eq!(decide(&facts), Reason::Run);
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

#[test]
fn gaming_holds_native_and_heavy_admissions_and_lets_light_work_run() {
    for class in [Class::Native, Class::Heavy, Class::Exclusive, Class::Gpu] {
        assert_eq!(decide(&Facts { gaming: true, ..facts(class) }), Reason::DeferGaming, "{class:?}");
        assert_eq!(decide(&Facts { gaming: false, ..facts(class) }), Reason::Run, "{class:?}");
    }
    for class in [Class::Agent, Class::Moderate, Class::Critical, Class::Tom] {
        assert_eq!(decide(&Facts { gaming: true, ..facts(class) }), Reason::Run, "{class:?}");
    }
}

#[test]
fn gaming_holds_a_landing_check_by_the_class_it_asked_for() {
    let landing = Facts { gaming: true, gaming_class: Some(Class::Heavy), ..facts(Class::Critical) };
    assert_eq!(decide(&landing), Reason::DeferGaming);
    assert_eq!(decide(&Facts { gaming: false, ..landing }), Reason::Run);
}

#[test]
fn gaming_deferred_tickets_ahead_do_not_queue_light_work_behind_them() {
    let ahead = |gaming, classes: &[Class]| Facts { gaming, queued_ahead: 2, queued_ahead_classes: classes.iter().copied().map(Some).collect(), ..facts(Class::Moderate) };
    assert_eq!(decide(&ahead(true, &[Class::Heavy, Class::Gpu])), Reason::Run);
    assert_eq!(decide(&ahead(true, &[Class::Heavy, Class::Moderate])), Reason::DeferQueued);
    assert_eq!(decide(&ahead(false, &[Class::Heavy, Class::Gpu])), Reason::DeferQueued);
    assert_eq!(decide(&ahead(true, &[Class::Critical, Class::Tom])), Reason::DeferQueued);
}
