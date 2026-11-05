#![cfg(test)]

use super::admission::{Class, Facts, Reason, decide};
use super::native_hold::LeaseFact;
use super::tests::{facts, running};

// KEEP(bug): the next heavy start waits 30 s because a start's pressure arrives after 10 s; regression of north#400 (gex45 committed 30 CPUs on 20 cores)
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
