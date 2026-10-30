use super::admission::{Class, Facts, Profile, Reason, admit, decide};
use super::tests::{facts, overload};

#[test]
fn gpu_work_waits_for_a_busy_laptop_gpu_while_tom_is_present_and_names_the_host_that_can_take_it() {
    let admitted = |class: &str, need: Option<&str>, profile: Profile, busy: u32, gpu_runs: u32| {
        let (mut facts, hosts) = overload(class, need);
        (facts.profile, facts.gpu_busy_percent, facts.gpu_runs, facts.run_queue_load) = (profile, busy, gpu_runs, Some(0.0));
        admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "here"))
    };
    let native = Some("native:smashcraft");
    let deferred = admitted("heavy", native, Profile::Attended, 100, 0);
    assert_eq!((deferred.reason, deferred.offload.as_deref()), (Reason::DeferGpuBusy, Some("gpu-host")));
    assert_eq!(admitted("gpu", None, Profile::Attended, 100, 0).reason, Reason::DeferGpuBusy);
    assert_eq!(admitted("heavy", native, Profile::Attended, 84, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", native, Profile::Unattended, 100, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", None, Profile::Attended, 100, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", native, Profile::Attended, 100, 1).reason, Reason::Run);
}

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
    assert_eq!(saturated(Class::Moderate, 6, 0), Reason::DeferCpuCapacity);
    assert_eq!(saturated(Class::Heavy, 6, 0), Reason::DeferRunQueue);
}

#[test]
fn a_tom_class_request_is_admitted_while_the_busy_gpu_defers_native_and_gpu_work() {
    let admitted = |class: &str, need: Option<&str>| {
        let (mut facts, hosts) = overload(class, need);
        (facts.profile, facts.gpu_busy_percent, facts.gpu_runs, facts.run_queue_load) = (Profile::Attended, 100, 0, Some(0.0));
        admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "here")).reason
    };
    let native = Some("native:smashcraft");
    assert_eq!(admitted("tom", native), Reason::Run);
    assert_eq!(admitted("tom", None), Reason::Run);
    assert_eq!(admitted("heavy", native), Reason::DeferGpuBusy);
    assert_eq!(admitted("gpu", None), Reason::DeferGpuBusy);
}
