use super::policy::{charged_resources, memory_margin};
use super::sizing::sizing_runs;

mod properties {
    use super::super::policy::{Resources, charged_resources, clamp_to_grantable, memory_margin, resource_class, running_cpu_commitment, scope_cpu_quota};
    use proptest::prelude::*;
    use super::super::json::num;

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
        // KEEP(mutant): a clamp never grants past the request or the maximum and reports exactly the fields it cut
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

        // KEEP(mutant): the charged size is declared until three runs then nearest-rank p90 within bounds, and never exceeds the scope quota
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

// KEEP(bug): a tom request with few runs of its shape is sized from its whole class, not the flat 16 GiB; regression of 248250f80
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

// KEEP(bug): a tiny host still grants one core so the aggregate slice never gets a zero quota; regression of 417cb70da
#[test]
fn a_tiny_host_still_grants_one_core_so_the_aggregate_slice_never_gets_a_zero_quota() {
    use crate::capacity::cpus::aggregate_cpus;
    assert_eq!(aggregate_cpus("attended", 1.0, None), 1.0);
    assert_eq!(aggregate_cpus("attended", 2.0, None), 1.0);
    assert_eq!(aggregate_cpus("attended", 20.0, None), 16.0);
    assert_eq!(aggregate_cpus("unattended", 1.0, None), 1.0);
}

// KEEP(bug): a heavy shape whose twenty measured runs all peaked under its class is charged its highest peak; regression of 1322d2924
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
