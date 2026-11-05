#![cfg(test)]

use super::admission::{Class, Facts, Reason, admit, decide};
use serde_json::json;

use super::native_hold::{PairLease, PairRefusal, QuietNative, queue_pair_leases, standing_json};
use super::tests::facts;

fn quiet(owner: &str, held_seconds: f64, idle_seconds: Option<f64>, run_seconds: Option<f64>) -> QuietNative {
    QuietNative { owner: owner.to_owned(), held_seconds, idle_seconds, run_seconds }
}

#[test]
fn a_quiet_lease_idle_past_its_bound_or_past_its_declared_run_stops_deferring_batch_work_while_a_busy_one_still_does() {
    let batch = |lease: QuietNative| decide(&Facts { quiet_native: vec![lease], ..facts(Class::Moderate) });
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

#[test]
fn a_quiet_native_lease_holds_a_second_native_or_gpu_lease_until_it_is_released_and_names_the_holder() {
    let holder = QuietNative { owner: "flash-gap-244".to_owned(), held_seconds: 60.0, idle_seconds: None, run_seconds: Some(3600.0) };
    for class in [Class::Native, Class::Gpu] {
        let asked = |quiet_native: Vec<QuietNative>, owner: &str| admit(&Facts { quiet_native, owner: Some(owner.to_owned()), ..facts(class) }, |_| None);
        let waiting = asked(vec![holder.clone()], "part9a-1122");
        assert_eq!((waiting.reason, waiting.quiet_owners), (Reason::DeferNativeQuiet, vec!["flash-gap-244".to_owned()]), "{class:?}");
        assert_eq!(asked(Vec::new(), "part9a-1122").reason, Reason::Run, "{class:?} after release");
        assert_eq!(asked(vec![holder.clone()], "flash-gap-244").reason, Reason::Run, "{class:?} the holder's own steps");
    }
}

#[test]
fn status_names_the_quiet_holder_and_the_native_and_batch_owners_waiting_on_it() {
    let lease = |class: &str, quiet: bool, owner: &str| json!({ "id": format!("{owner}-lease"), "class": class, "owner": owner, "quiet": quiet, "createdAt": 0.0, "quietSeconds": 3600.0 });
    let ticket = |class: &str, owner: &str, reason: &str| json!({ "name": owner, "class": class, "owner": owner, "reason": reason, "queuedAt": 20_000.0 });
    let leases = [lease("native", true, "flash-gap-244")];
    let queue = [ticket("native", "part9a-1122", "DEFER_NATIVE_QUIET"), ticket("native", "other", "DEFER_NATIVE_CPUS"), ticket("moderate", "build-7", "DEFER_NATIVE_QUIET")];
    let standing = standing_json(&leases, &queue, 60_000.0);
    assert_eq!(standing[0]["owner"], "flash-gap-244");
    assert_eq!(standing[0]["deferring"], json!({ "jobs": 2, "longestWaitingSeconds": 40.0, "waiters": ["part9a-1122", "build-7"] }));
}

#[test]
fn a_pair_the_wisp_queue_holds_for_a_live_process_is_refused_to_a_native_lease_with_a_different_free_pair() {
    let directory = std::env::temp_dir().join(format!("capture-queue-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("leases")).unwrap();
    let write = |name: &str, host: &str, pair: u32, pid: u32| std::fs::write(directory.join("leases").join(name), json!({ "host": host, "pair": pair, "owner": "wisp-sylv-cloak", "pid": pid, "since": "" }).to_string()).unwrap();
    write("gex45-16.json", "gex45", 16, std::process::id());
    write("gex45-17.json", "gex45", 17, u32::MAX);
    write("elsewhere-18.json", "elsewhere", 18, std::process::id());
    let held = queue_pair_leases(&directory, "gex45");
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(held.iter().map(|lease| (lease.pair, lease.owner.as_str())).collect::<Vec<_>>(), vec![(16, "wisp-sylv-cloak")]);
    let ask = |pair: u32| Facts { owner: Some("sc-lk-gale".to_owned()), pair: Some(pair), pair_leases: held.clone(), ..facts(Class::Native) };
    let refused = admit(&ask(16), |_| None);
    assert_eq!(refused.reason, Reason::PairHeld);
    assert_eq!(refused.pair, Some(PairRefusal { pair: 16, holder: "wisp-sylv-cloak".to_owned(), free_pair: 0 }));
    assert_eq!(decide(&ask(0)), Reason::Run);
}
