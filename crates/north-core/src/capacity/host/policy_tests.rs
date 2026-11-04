use super::policy::{charged_resources, memory_margin, resource_class, running_memory_charge};
use super::sizing::sizing_runs;

mod properties {
    use super::super::policy::{Resources, aggregate_cpus, clamp_to_grantable, reserved_cpus};
    use proptest::prelude::*;
    use super::super::json::num;

    fn profile() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("attended"), Just("unattended")]
    }

    fn amount(most: u32) -> impl Strategy<Value = f64> {
        prop_oneof![(0..=most).prop_map(f64::from), 0.0..f64::from(most)]
    }

    fn resources() -> impl Strategy<Value = Resources> {
        (amount(512), amount(1_048_576)).prop_map(|(cpus, memory_mib)| Resources { cpus, memory_mib })
    }

    proptest! {
        #[test]
        fn prop_aggregate_cpus_is_at_least_one_and_at_most_the_cores(profile in profile(), cores in 1u32..=256) {
            let aggregate = aggregate_cpus(profile, f64::from(cores));
            prop_assert!(aggregate >= 1.0, "{profile} on {cores} cores grants {aggregate} CPUs");
            prop_assert!(aggregate <= f64::from(cores), "{profile} on {cores} cores grants {aggregate} CPUs");
        }

        #[test]
        fn prop_reserved_and_aggregate_cpus_sum_to_the_cores_wherever_the_reserve_leaves_a_core(profile in profile(), cores in 1u32..=256) {
            let cores = f64::from(cores);
            let reserved = reserved_cpus(profile, cores);
            prop_assume!(cores - reserved >= 1.0);
            prop_assert_eq!(reserved + aggregate_cpus(profile, cores), cores);
        }

        #[test]
        fn prop_clamp_to_grantable_never_grants_past_the_request_or_maximum_and_reports_exactly_the_fields_it_cut(request in resources(), maximum in resources()) {
            let (granted, clamped) = clamp_to_grantable(request, maximum);
            prop_assert!(granted.cpus <= request.cpus && granted.cpus <= maximum.cpus);
            prop_assert!(granted.memory_mib <= request.memory_mib && granted.memory_mib <= maximum.memory_mib);
            let cut = |field: &str| clamped.as_ref().and_then(|value| value.get(field)).cloned();
            let expected = |requested: f64, given: f64| (given != requested).then(|| serde_json::Value::Array(vec![num(requested), num(given)]));
            prop_assert_eq!(cut("cpus"), expected(request.cpus, granted.cpus));
            prop_assert_eq!(cut("memoryMiB"), expected(request.memory_mib, granted.memory_mib));
            prop_assert_eq!(clamped.is_some(), granted != request);
            prop_assert_eq!(clamped.as_ref().and_then(|value| value.as_object()).map_or(0, serde_json::Map::len), usize::from(granted.cpus != request.cpus) + usize::from(granted.memory_mib != request.memory_mib));
        }
    }
}

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
fn a_running_tom_critical_moderate_or_gpu_lease_past_the_ramp_holds_its_measured_peak_up_to_its_charge() {
    for kind in ["tom", "critical", "moderate", "gpu"] {
        assert!((running_memory_charge(kind, 16384.0, 424.0, Some(2832.0)) - 2832.0 * memory_margin()).abs() < 1e-9, "{kind} past the ramp");
        assert_eq!(running_memory_charge(kind, 16384.0, 30.0, Some(158.0)), 16384.0, "{kind} inside the ramp keeps its charge");
        assert_eq!(running_memory_charge(kind, 16384.0, 424.0, None), 16384.0, "{kind} with no measurement keeps its charge");
        assert_eq!(running_memory_charge(kind, 16384.0, 424.0, Some(20000.0)), 16384.0, "{kind} never holds past its charge");
    }
    for kind in ["heavy", "exclusive", "runtime", "native"] {
        assert_eq!(running_memory_charge(kind, 8192.0, 424.0, Some(900.0)), 8192.0, "{kind} keeps its charge");
    }
}

#[test]
fn a_tom_request_with_too_few_runs_of_its_shape_is_sized_from_its_whole_class() {
    let run = |peak: f64| serde_json::json!({ "class": "tom", "p90Cores": 4.0, "peakMemoryMiB": peak, "wallSeconds": 60 });
    let class_runs = || [2357.0, 84.0, 4229.0, 3379.0, 2.0, 191.0].map(run).to_vec();
    let (runs, class_wide) = sizing_runs("tom", vec![run(2357.0)], class_runs);
    assert!(class_wide);
    let memory: Vec<f64> = runs.iter().map(|run| run["peakMemoryMiB"].as_f64().unwrap_or(f64::NAN)).collect();
    let charged = charged_resources("tom", 20.0, &vec![4.0; memory.len()], &memory).expect("charged");
    assert!((charged.memory_mib - 4229.0 * memory_margin()).abs() < 1e-9, "the class's p90 peak with margin, not 16 GiB");
    let (own, class_wide) = sizing_runs("tom", vec![run(900.0), run(900.0), run(900.0)], class_runs);
    assert!(!class_wide && own.len() == 3, "three runs of its own shape size it");
    let (heavy, class_wide) = sizing_runs("heavy", vec![run(900.0)], class_runs);
    assert!(!class_wide && heavy.len() == 1, "other classes never borrow class runs");
}

#[test]
fn a_tiny_host_still_grants_one_core_so_the_aggregate_slice_never_gets_a_zero_quota() {
    use super::policy::aggregate_cpus;
    assert_eq!(aggregate_cpus("attended", 1.0), 1.0);
    assert_eq!(aggregate_cpus("attended", 2.0), 1.0);
    assert_eq!(aggregate_cpus("attended", 20.0), 16.0);
    assert_eq!(aggregate_cpus("unattended", 1.0), 1.0);
}

#[test]
fn a_frozen_batch_lease_is_still_charged_its_memory_and_reserves_no_batch_cpus() {
    let root = tempfile::tempdir().unwrap();
    let leases = [
        serde_json::json!({ "id": "a", "kind": "run", "class": "heavy", "cpus": 6, "memoryMiB": 8192, "createdAt": 0, "frozenAt": 1000 }),
        serde_json::json!({ "id": "b", "kind": "run", "class": "moderate", "cpus": 2, "memoryMiB": 2048, "createdAt": 0 }),
        serde_json::json!({ "id": "c", "kind": "run", "class": "heavy", "landing": true, "cpus": 6, "memoryMiB": 8192, "createdAt": 0 }),
        serde_json::json!({ "id": "d", "kind": "run", "class": "tom", "cpus": 20, "memoryMiB": 16384, "createdAt": 0 }),
    ];
    let totals = super::request::totals(root.path(), &leases, 600_000.0);
    assert_eq!(totals.memory_mib, 8192.0 + 2048.0 + 8192.0 + 16384.0, "freezing frees CPU, not memory");
    let accounting = super::request::cpu_accounting(root.path(), &leases, 600_000.0);
    let leased = accounting.committed_batch_cpus - accounting.unleased_cores().unwrap_or(0.0);
    assert!((leased - 26.0).abs() < 0.02, "only the landing check and Tom's build commit CPUs: {leased}");
    assert_eq!(accounting.committed_elastic_cpus, 8.0, "the idle-weighted leases are counted apart");
}
