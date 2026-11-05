#![cfg(test)]

use super::admission::{Class, Facts, Profile, Reason, decide};
use super::build_offload::Admission;
use super::fixtures::facts;
use super::land_claim;

fn gex45(land_check_waiting: bool, leased_batch_cpus: f64, queued_ahead: u32) -> impl Fn(Class) -> Facts {
    move |class| Facts { profile: Profile::Unattended, cores: 20, leased_batch_cpus, requested_cpus: 6.0, queued_ahead, land_check_waiting, ..facts(class) }
}

fn status(committed_cpus: f64, cpu_pressure_percent: f64, land_check_waiting: bool) -> Admission {
    Admission { limit_cpus: 20.0, committed_cpus, cpu_pressure_percent, pressure_high_percent: 40.0, land_check_waiting }
}

#[test]
fn a_waiting_land_check_takes_the_cores_worker_builds_free_before_any_queued_worker_build() {
    let open = gex45(false, 2.0, 0);
    assert_eq!(decide(&open(Class::Heavy)), Reason::Run, "with no land check waiting the host admits a worker build");

    let claimed = gex45(true, 2.0, 0);
    for class in [Class::Heavy, Class::Moderate, Class::Exclusive] {
        assert_eq!(decide(&claimed(class)), Reason::DeferLandCheckWaiting, "{class:?} lease while a land check waits");
    }
    let queued = gex45(true, 2.0, 3);
    assert_eq!(decide(&queued(Class::Heavy)), Reason::DeferLandCheckWaiting, "a worker build queued first still yields");
    assert_eq!(decide(&queued(Class::Critical)), Reason::Run, "the land check itself runs as critical, ahead of the queue");
    for class in [Class::Agent, Class::Gpu, Class::Tom, Class::Runtime] {
        assert_ne!(decide(&claimed(class)), Reason::DeferLandCheckWaiting, "{class:?} keeps its own rules");
    }
    assert_eq!(serde_json::to_value(Reason::DeferLandCheckWaiting).unwrap(), "DEFER_LAND_CHECK_WAITING");

    let filled = status(19.0, 51.0, true);
    assert!(filled.shortfall(6.0, 0.0).is_some(), "the 3:15 AM gex45: worker builds hold the cores and pressure");
    let drained = status(2.0, 12.0, true);
    assert_eq!(drained.shortfall(6.0, 0.0), None, "once running builds finish the land check fits");
    assert_eq!(decide(&gex45(true, 2.0, 1)(Class::Heavy)), Reason::DeferLandCheckWaiting, "while the queued worker build still waits");
    assert_eq!(land_claim::REASON, "land check waiting");
}

#[test]
fn a_claim_lapses_after_its_seconds_so_a_stopped_lander_frees_the_host() {
    let seconds = super::admission::default_mib("land_check_claim_seconds") * 1000.0;
    assert!(land_claim::stands(1_000.0, 1_000.0 + seconds - 1.0));
    assert!(!land_claim::stands(1_000.0, 1_000.0 + seconds));
}

#[test]
fn a_host_status_carries_the_claim_to_worker_build_placement() {
    let read = |claim: serde_json::Value| Admission::from_status(&serde_json::json!({"profile": "unattended", "aggregateCpuLimit": 20, "committedBatchCpus": 2, "cpuSomeAvg10": 5, "landCheckWaiting": claim})).unwrap();
    assert!(read(true.into()).land_check_waiting);
    assert!(!read(false.into()).land_check_waiting);
    assert!(!read(serde_json::Value::Null).land_check_waiting, "an older runtime's status has no claim");
}
