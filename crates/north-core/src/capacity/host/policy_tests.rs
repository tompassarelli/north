use super::policy::{charged_resources, memory_margin, resource_class, running_memory_charge};
use super::sizing::sizing_runs;

mod properties {
    use super::super::policy::{Resources, aggregate_cpus, charged_resources, clamp_to_grantable, memory_margin, reserved_cpus, resource_class, running_cpu_commitment, scope_cpu_quota};
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

    fn kind() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec!["moderate", "heavy", "native", "gpu", "exclusive", "critical", "tom", "agent"])
    }

    fn measured_runs() -> impl Strategy<Value = Vec<(f64, f64)>> {
        prop::collection::vec(((0u32..=1200).prop_map(|cents| f64::from(cents) / 100.0), (0u32..=20000).prop_map(f64::from)), 0..=20)
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

        #[test]
        fn prop_a_class_is_charged_its_declared_size_until_three_runs_then_their_nearest_rank_p90_within_its_bounds(kind in kind(), cores in 1u32..=32, runs in measured_runs()) {
            let cores = f64::from(cores);
            let (run_cores, run_memory): (Vec<f64>, Vec<f64>) = runs.into_iter().unzip();
            let declared = resource_class(kind, cores).expect("declared class");
            let charged = charged_resources(kind, cores, &run_cores, &run_memory).expect("charged");
            let nearest = |values: &[f64]| { let mut sorted = values.to_vec(); sorted.sort_by(f64::total_cmp); sorted[(sorted.len() as f64 * 0.9).ceil() as usize - 1] };
            let measured = run_cores.len() >= 3;
            let expected = match kind {
                "tom" | "critical" if measured => Resources { cpus: declared.cpus, memory_mib: (nearest(&run_memory) * memory_margin()).min(declared.memory_mib) },
                "moderate" | "heavy" | "native" | "gpu" if measured => {
                    let cpus = nearest(&run_cores).max(1.0).min(declared.cpus * 1.5);
                    Resources { cpus: if kind == "heavy" { cpus.max(declared.cpus) } else { cpus }, memory_mib: nearest(&run_memory) * memory_margin() }
                }
                _ => declared,
            };
            prop_assert_eq!(charged.cpus, expected.cpus);
            prop_assert!((charged.memory_mib - expected.memory_mib).abs() < 1e-9, "{} charged {} MiB, expected {}", kind, charged.memory_mib, expected.memory_mib);
            let measured_cores = run_cores.first().copied().unwrap_or(0.0);
            prop_assert_eq!(running_cpu_commitment("native", Some(measured_cores), Some(declared.cpus), declared.cpus, None).0, measured_cores * 1.25);
            prop_assert_eq!(running_cpu_commitment("heavy", Some(measured_cores), Some(1.0), declared.cpus, Some(declared.cpus)).0, declared.cpus);
            if let Some(quota) = scope_cpu_quota(kind, charged.cpus, Some(declared.cpus)) {
                prop_assert!(running_cpu_commitment(kind, Some(measured_cores), run_cores.last().copied(), charged.cpus, Some(quota)).0 <= quota);
            }
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

#[test]
fn a_measured_gpu_request_sizes_its_cpus_and_memory_and_native_sizes_only_memory() {
    use super::policy::{Resources, measured_request};
    assert_eq!(measured_request("gpu", 16.0, Some(4.0), Some(4.0)), Ok(Resources { cpus: 4.0, memory_mib: 4096.0 }));
    assert_eq!(measured_request("gpu", 16.0, None, Some(3.0)), Ok(Resources { cpus: 3.0, memory_mib: 2048.0 }));
    assert_eq!(measured_request("native", 16.0, Some(1.5), None), Ok(Resources { cpus: 2.0, memory_mib: 1536.0 }));
    assert!(measured_request("native", 16.0, None, Some(4.0)).is_err());
    assert!(measured_request("heavy", 16.0, Some(4.0), None).is_err());
    assert!(measured_request("gpu", 16.0, None, Some(17.0)).is_err());
    assert!(measured_request("gpu", 16.0, None, Some(1.5)).is_err());
    assert!(measured_request("gpu", 16.0, Some(0.0), None).is_err());
}

#[test]
fn a_history_sized_request_past_a_four_core_hosts_cores_is_granted_all_four() {
    use super::policy::{Resources, clamp_to_grantable, grantable_maximum};
    let (granted, clamped) = clamp_to_grantable(Resources { cpus: 4.33_f64.ceil(), memory_mib: 2429.0 }, grantable_maximum("unattended", 4.0, 16384.0));
    assert_eq!((granted, clamped), (Resources { cpus: 4.0, memory_mib: 2429.0 }, Some(serde_json::json!({ "cpus": [5, 4] }))));
}

#[test]
fn a_heavy_shape_whose_twenty_measured_runs_all_peaked_under_its_class_is_charged_its_highest_peak() {
    use super::policy::heavy_peak_charge;
    let mut land_checks = vec![2.4; 39];
    land_checks.push(4.0);
    assert_eq!(heavy_peak_charge(6.0, &land_checks), Some(4.0), "the 2026-11-05 land-check runs fit five checks under the 20-CPU line, not three");
    assert_eq!(heavy_peak_charge(6.0, &land_checks[..19]), None, "nineteen runs keep the declared six cores");
    let mut burst = land_checks.clone();
    burst.push(6.5);
    assert_eq!(heavy_peak_charge(6.0, &burst), None, "a shape that ever peaked at its class keeps the class");
    let mut unmeasured = land_checks.clone();
    unmeasured.push(f64::NAN);
    assert_eq!(heavy_peak_charge(6.0, &unmeasured), None, "a run without a measured peak keeps the class");
    assert_eq!(heavy_peak_charge(6.0, &[0.0; 20]), None, "a run without a positive peak keeps the class");
    assert_eq!(heavy_peak_charge(6.0, &[0.6; 20].into_iter().chain([0.8]).collect::<Vec<_>>()), Some(1.0), "a tiny shape is charged at least one core");
}

#[test]
fn a_land_check_asks_a_check_host_for_its_measured_peak_once_its_newest_runs_are_measured() {
    use super::land_check_charge;
    let run = |peak: f64| serde_json::json!({ "owner": "land-check", "class": "heavy", "peakCores": peak, "wallSeconds": 121.0 });
    let mut runs: Vec<serde_json::Value> = (0..30).map(|_| run(9.0)).collect();
    runs.extend((0..40).map(|index| run(if index == 7 { 4.0 } else { 2.4 })));
    assert_eq!(land_check_charge(6.0, &runs), 4.0, "older runs past the newest forty do not hold the charge at the class");
    assert_eq!(land_check_charge(6.0, &runs[..60]), 6.0, "a peak at or over the class among the newest forty keeps six");
    assert_eq!(land_check_charge(6.0, &runs[..19]), 6.0, "too few runs keep six");
}
