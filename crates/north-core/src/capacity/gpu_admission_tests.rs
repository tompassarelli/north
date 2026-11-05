use super::admission::{Class, Facts, Profile, Reason, decide};
use super::tests::facts;

#[test]
fn a_gpu_lease_is_admitted_on_gpu_occupancy_while_the_cpu_is_saturated_by_other_work() {
    let saturated = |class: Class, gpu_busy_percent: u32, gpu_runs: u32| {
        decide(&Facts {
            profile: Profile::Unattended,
            run_queue_load: Some(25.0),
            cpu_some_avg10_basis_points: 4500,
            cpu_pressure_measured: true,
            leased_batch_cpus: 22.0,
            unleased_heavy_cores: 3.0,
            gpu_busy_percent,
            gpu_runs,
            ..facts(class)
        })
    };
    assert_eq!(saturated(Class::Gpu, 6, 0), Reason::Run);
    assert_eq!(saturated(Class::Gpu, 6, 1), Reason::Run);
    assert_eq!(saturated(Class::Gpu, 6, 2), Reason::DeferGpuSlots);
    assert_eq!(saturated(Class::Moderate, 6, 0), Reason::DeferCpuPressure);
    assert_eq!(saturated(Class::Heavy, 6, 0), Reason::DeferRunQueue);
}
