use super::placement::gate::serving;
use crate::capacity::admission::{Admission, Facts, Profile, Reason, admit};

const OVERLOAD: &str = include_str!("../../../../harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json");

fn overload(class: &str, need: Option<&str>) -> (Facts, toml_edit::DocumentMut) {
    let recorded: serde_json::Value = serde_json::from_str(OVERLOAD).unwrap();
    let mut facts = recorded["facts"].clone();
    facts["class"] = class.into();
    facts["need"] = need.into();
    (serde_json::from_value(facts).unwrap(), recorded["hosts"].as_str().unwrap().parse().unwrap())
}

fn overload_admission(class: &str, need: Option<&str>) -> Admission {
    let (facts, hosts) = overload(class, need);
    admit(&facts, |needs| serving(&hosts, needs, "here"))
}

#[test]
fn run_queue_over_thread_count_refuses_heavy_and_exclusive_naming_the_serving_host() {
    for class in ["heavy", "exclusive"] {
        let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned), aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() };
        assert_eq!(overload_admission(class, Some("native:makgora")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:makgora")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:galileo")), refused(None), "{class}");
        assert_eq!(overload_admission(class, None), refused(None), "{class}");
    }
}

#[test]
fn run_queue_over_thread_count_still_admits_moderate_critical_and_tom() {
    for class in ["moderate", "critical", "tom"] {
        assert_eq!(overload_admission(class, Some("native:makgora")), Admission { reason: Reason::Run, offload: None, aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() }, "{class}");
    }
}

#[test]
fn run_queue_offload_names_only_another_host_that_serves_every_need() {
    let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned), aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() };
    assert_eq!(overload_admission("heavy", Some("native:makgora,private:makgora")), refused(Some("gpu-host")));
    assert_eq!(overload_admission("heavy", Some("native:makgora,private:galileo")), refused(None));
    let (facts, hosts) = overload("heavy", Some("native:makgora"));
    assert_eq!(admit(&facts, |needs| serving(&hosts, needs, "gpu-host")), refused(None));
}

#[test]
fn gpu_work_waits_for_a_busy_laptop_gpu_while_tom_is_present_and_names_the_host_that_can_take_it() {
    let admitted = |class: &str, need: Option<&str>, profile: Profile, busy: u32, gpu_runs: u32| {
        let (mut facts, hosts) = overload(class, need);
        (facts.profile, facts.gpu_busy_percent, facts.gpu_runs, facts.run_queue_load) = (profile, busy, gpu_runs, Some(0.0));
        admit(&facts, |needs| serving(&hosts, needs, "here"))
    };
    let native = Some("native:makgora");
    let deferred = admitted("heavy", native, Profile::Attended, 100, 0);
    assert_eq!((deferred.reason, deferred.offload.as_deref()), (Reason::DeferGpuBusy, Some("gpu-host")));
    assert_eq!(admitted("gpu", None, Profile::Attended, 100, 0).reason, Reason::DeferGpuBusy);
    assert_eq!(admitted("heavy", native, Profile::Attended, 84, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", native, Profile::Unattended, 100, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", None, Profile::Attended, 100, 0).reason, Reason::Run);
    assert_eq!(admitted("heavy", native, Profile::Attended, 100, 1).reason, Reason::Run);
}

#[test]
fn a_tom_class_request_is_admitted_while_the_busy_gpu_defers_native_and_gpu_work() {
    let admitted = |class: &str, need: Option<&str>| {
        let (mut facts, hosts) = overload(class, need);
        (facts.profile, facts.gpu_busy_percent, facts.gpu_runs, facts.run_queue_load) = (Profile::Attended, 100, 0, Some(0.0));
        admit(&facts, |needs| serving(&hosts, needs, "here")).reason
    };
    let native = Some("native:makgora");
    assert_eq!(admitted("tom", native), Reason::Run);
    assert_eq!(admitted("tom", None), Reason::Run);
    assert_eq!(admitted("heavy", native), Reason::DeferGpuBusy);
    assert_eq!(admitted("gpu", None), Reason::DeferGpuBusy);
}
