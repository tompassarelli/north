use super::admission::{Class, Facts, Reason, admit, decide};
use super::native_hold::LeaseFact;
use super::tests::{facts, running};

#[test]
fn heavy_admission_reads_pressure_and_the_settle_window_and_never_the_declared_cpus() {
    let heavy = |game_cpus| Facts { leased_batch_cpus: 14.0, requested_cpus: 6.0, game_reserve_agent_cpus: game_cpus, memory_available_mib: 80000.0, ..facts(Class::Heavy) };
    assert_eq!(decide(&heavy(None)), Reason::Run);
    assert_eq!(decide(&heavy(Some(16))), Reason::Run);
    let busy = Facts { cpu_some_avg10_basis_points: 6570, cpu_pressure_measured: true, memory_available_mib: 48000.0, need: Some("native:makgora".into()), ..facts(Class::Heavy) };
    let advice = admit(&busy, |needs| (needs == "native:makgora").then(|| "gex45".to_owned()));
    assert_eq!((advice.reason, advice.offload.as_deref()), (Reason::DeferRunQueue, Some("gex45")));
    let next = |age_seconds: f64| Facts { now: 100_000.0, leases: vec![LeaseFact { created_at: 100_000.0 - age_seconds * 1000.0, ..running("heavy") }], ..facts(Class::Heavy) };
    assert_eq!(decide(&next(9.0)), Reason::DeferCpuPressure);
    assert_eq!(decide(&next(29.0)), Reason::DeferCpuPressure);
    assert_eq!(decide(&next(31.0)), Reason::Run);
    let at = |basis_points, leased| Facts { cpu_some_avg10_basis_points: basis_points, leased_batch_cpus: leased, requested_cpus: 6.0, ..facts(Class::Heavy) };
    assert_eq!(decide(&at(500, 14.0)), Reason::Run);
    assert_eq!(decide(&at(1500, 10.0)), Reason::Run);
    assert_eq!(decide(&at(1500, 20.0)), Reason::Run);
    assert_eq!(decide(&at(500, 30.0)), Reason::Run);
    assert_eq!(decide(&at(2499, 60.0)), Reason::Run);
    assert_eq!(decide(&at(2500, 0.0)), Reason::DeferCpuPressure);
}

#[test]
fn a_start_whose_pressure_arrives_after_ten_seconds_holds_the_next_start_until_thirty() {
    let final_basis_points = 3000.0;
    let reading = |age_seconds: f64| (final_basis_points * (1.0 - (-age_seconds / 10.0).exp())) as u32;
    let next = |age_seconds: f64| Facts {
        now: 100_000.0,
        leases: vec![LeaseFact { created_at: 100_000.0 - age_seconds * 1000.0, ..running("heavy") }],
        cpu_some_avg10_basis_points: reading(age_seconds),
        cpu_pressure_measured: true,
        ..facts(Class::Heavy)
    };
    assert!(reading(10.0) < 2000, "ten seconds in the average has shown only 63% of the final pressure");
    assert_ne!(decide(&next(10.0)), Reason::Run);
    assert_ne!(decide(&next(20.0)), Reason::Run);
    assert!(reading(30.0) > 2800, "thirty seconds in the average shows 95% of the final pressure");
    assert_eq!(decide(&Facts { cpu_some_avg10_basis_points: 3000, ..next(31.0) }), Reason::DeferRunQueue);
    assert_eq!(decide(&Facts { cpu_some_avg10_basis_points: 100, ..next(31.0) }), Reason::Run);
}
