use super::admission::{Class, Facts, Reason, admit, decide};
use super::native_hold::{PairLease, PairRefusal, QuietNative};
use super::tests::facts;

fn quiet(owner: &str, held_seconds: f64, idle_seconds: Option<f64>, run_seconds: Option<f64>) -> QuietNative {
    QuietNative { owner: owner.to_owned(), held_seconds, idle_seconds, run_seconds }
}

#[test]
fn a_quiet_lease_idle_past_its_bound_or_past_its_declared_run_stops_deferring_batch_work_while_a_busy_one_still_does() {
    let batch = |lease: QuietNative| decide(&Facts { quiet_native: vec![lease], ..facts(Class::Heavy) });
    assert_eq!(batch(quiet("sc-434-t1915", 1020.0, None, None)), Reason::DeferNativeQuiet);
    assert_eq!(batch(quiet("sc-434-t1915", 1020.0, Some(10.0), None)), Reason::DeferNativeQuiet);
    assert_eq!(batch(quiet("sc-434-t1915", 1020.0, Some(1000.0), None)), Reason::Run);
    assert_eq!(batch(quiet("sc-434-t1915", 400.0, None, Some(600.0))), Reason::DeferNativeQuiet);
    assert_eq!(batch(quiet("sc-434-t1915", 700.0, None, Some(600.0))), Reason::Run);
}

#[test]
fn the_deferral_names_only_the_quiet_owners_that_still_hold() {
    let leases = vec![quiet("stale", 1020.0, Some(1000.0), None), quiet("busy", 1020.0, Some(5.0), None)];
    let admission = admit(&Facts { quiet_native: leases, ..facts(Class::Moderate) }, |_| None);
    assert_eq!((admission.reason, admission.quiet_owners), (Reason::DeferNativeQuiet, vec!["busy".to_owned()]));
}

#[test]
fn a_queued_alone_request_defers_every_new_native_lease_until_it_has_run_and_runs_once_no_client_is_live() {
    let native = |alone: bool, alone_queued_ahead: u32, gpu_clients: u32, new_clients: u32| {
        decide(&Facts { alone, alone_queued_ahead, gpu_clients, new_clients, ..facts(Class::Native) })
    };
    for new_clients in [0, 1, 2] {
        assert_eq!(native(false, 1, 0, new_clients), Reason::DeferNativeAlone, "{new_clients} new clients");
    }
    assert_eq!(native(false, 0, 1, 1), Reason::Run);
    assert_eq!(native(true, 0, 2, 0), Reason::DeferNativeAlone);
    assert_eq!(native(true, 0, 1, 0), Reason::DeferNativeAlone);
    assert_eq!(native(true, 0, 0, 2), Reason::Run);
    assert_eq!(native(true, 1, 0, 2), Reason::DeferNativeAlone);
}

#[test]
fn a_second_session_asking_for_a_pair_another_live_owner_holds_is_refused_naming_the_holder_and_a_free_pair() {
    let leases = vec![PairLease { pair: 0, owner: "wisp-lead".to_owned() }, PairLease { pair: 4, owner: "wisp-worker".to_owned() }, PairLease { pair: 1, owner: "wisp-lead".to_owned() }];
    let ask = |owner: &str, pair: u32| Facts { owner: Some(owner.to_owned()), pair: Some(pair), pair_leases: leases.clone(), ..facts(Class::Native) };
    let refused = admit(&ask("sc-46-place", 4), |_| None);
    assert_eq!(refused.reason, Reason::PairHeld);
    assert_eq!(refused.pair, Some(PairRefusal { pair: 4, holder: "wisp-worker".to_owned(), free_pair: 2 }));
    assert_eq!(decide(&ask("wisp-worker", 4)), Reason::Run);
    assert_eq!(decide(&ask("sc-46-place", 2)), Reason::Run);
}
