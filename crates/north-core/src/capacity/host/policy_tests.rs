use super::policy::{charged_resources, memory_margin, resource_class, running_memory_charge};

#[test]
fn tom_and_critical_keep_their_cores_and_are_charged_measured_memory_up_to_their_declared_size() {
    for kind in ["tom", "critical"] {
        let declared = resource_class(kind, 20.0).expect("declared class");
        let measured = charged_resources(kind, 20.0, &[3.0, 5.0, 4.0], &[3406.0, 1329.0, 2900.0]).expect("charged");
        assert_eq!(measured.cpus, declared.cpus, "{kind} keeps its declared cores");
        assert!((measured.memory_mib - 3406.0 * memory_margin()).abs() < 1e-9, "{kind} is charged its p90 memory with margin");
        let large = charged_resources(kind, 20.0, &[3.0, 5.0, 4.0], &[30000.0, 30000.0, 30000.0]).expect("charged");
        assert_eq!(large.memory_mib, declared.memory_mib, "{kind} is never charged past its declared memory");
        let unmeasured = charged_resources(kind, 20.0, &[3.0, 5.0], &[3406.0, 1329.0]).expect("charged");
        assert_eq!((unmeasured.cpus, unmeasured.memory_mib), (declared.cpus, declared.memory_mib), "{kind} is charged its declared size until three runs");
    }
}

#[test]
fn a_running_tom_or_critical_lease_past_the_ramp_holds_its_measured_peak_up_to_its_charge() {
    for kind in ["tom", "critical"] {
        assert!((running_memory_charge(kind, 16384.0, 424.0, Some(2832.0)) - 2832.0 * memory_margin()).abs() < 1e-9, "{kind} past the ramp");
        assert_eq!(running_memory_charge(kind, 16384.0, 30.0, Some(158.0)), 16384.0, "{kind} inside the ramp keeps its charge");
        assert_eq!(running_memory_charge(kind, 16384.0, 424.0, None), 16384.0, "{kind} with no measurement keeps its charge");
        assert_eq!(running_memory_charge(kind, 16384.0, 424.0, Some(20000.0)), 16384.0, "{kind} never holds past its charge");
    }
    assert_eq!(running_memory_charge("heavy", 8192.0, 424.0, Some(900.0)), 8192.0, "other classes keep their charge");
}
