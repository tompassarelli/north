use super::json::Obj;
use super::offering::{deferred_session_build, offers_before_deciding, session_build_refusal};
use super::request::Create;
use crate::capacity::build_offload::{Candidate, Declined, Job};
use crate::capacity::effects::Host;

fn never_pushed(_: &Candidate, _: &Job, _: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    unreachable!("the refusal decides nothing about pushing")
}

fn never_handed(_: &Candidate, _: &Job, _: std::os::unix::net::UnixStream, _: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    unreachable!("the refusal decides nothing about handing over")
}

fn laptop(own_build_refusal: fn() -> Option<String>) -> Host {
    Host {
        serving: |_| None,
        warcraft_clients: || None,
        bar1_margin_mib: || None,
        offload: |_| 2,
        build_offload: never_pushed,
        hand_over: never_handed,
        lease_builds: || false,
        own_build_refusal,
        reclaim_disk: || None,
        tend_disk: || {},
        keep_disk: || {},
        tend_slots: || {},
    }
}

// KEEP(bug): a fleet session's cargo on the laptop that no full build host took ran nowhere (exit 75) while the laptop sat at load 3 of 24 on 2026-11-06; it now waits in the laptop's own admission whenever the laptop has room beyond its reserves; north#634
#[test]
fn a_session_compile_no_build_host_took_waits_here_when_this_host_has_room_and_is_refused_only_when_it_has_none() {
    let create = Create { kind: "run", owner: "worker".to_owned(), timeout_seconds: Some(60) };
    let cargo = ["cargo".to_owned(), "test".to_owned()];
    let full = Declined::NoRoom(vec!["gex45: land check waiting".to_owned(), "ccx33: land check waiting".to_owned()]);
    assert!(session_build_refusal(&laptop(|| None), &cargo, Some(&full), &create).is_none(), "room here: the compile queues in this host's admission");
    let refused = session_build_refusal(&laptop(|| Some("2.0 cores free of 14, needs 4".to_owned())), &cargo, Some(&full), &create).expect("no room anywhere");
    let detail = refused.get("detail").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    assert!(detail.contains("gex45: land check waiting") && detail.contains("this host: 2.0 cores free of 14"), "{detail}");
    assert!(session_build_refusal(&laptop(|| Some("full".to_owned())), &cargo, None, &create).is_none(), "a build host took it or was never asked");
}

// KEEP(bug): a fleet compile the laptop took for want of a build host queued in its admission for up to the gate's hour when admission deferred it, so a check with a 300 s deadline hung past it (lander, 2026-11-06 5:55 PM); a deferral here now refuses it at once; north#634
#[test]
fn a_session_compile_this_host_took_is_refused_at_once_when_its_admission_defers_it_never_queued() {
    let create = Create { kind: "run", owner: "worker".to_owned(), timeout_seconds: Some(3600) };
    let cargo = ["cargo".to_owned(), "test".to_owned()];
    let full = Declined::NoRoom(vec!["gex45: land check waiting".to_owned()]);
    let deferred = Obj::new().put("decision", "DEFER").put("reason", "DEFER_QUEUED");
    let refused = deferred_session_build(&laptop(|| None), &cargo, Some(&full), &create, &deferred).expect("a deferral refuses it");
    assert_eq!(refused.get("decision").and_then(serde_json::Value::as_str), Some("REFUSED"));
    let detail = refused.get("detail").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    assert!(detail.contains("gex45: land check waiting") && detail.contains("DEFER_QUEUED"), "{detail}");
    let admitted = Obj::new().put("decision", "RESERVED").put("reason", "RUN");
    assert!(deferred_session_build(&laptop(|| None), &cargo, Some(&full), &create, &admitted).is_none(), "admitted now: it runs here");
    assert!(deferred_session_build(&laptop(|| None), &cargo, None, &create, &deferred).is_none(), "not taken for want of a build host: it queues as before");
}

// KEEP(bug): with two rented build hosts gone on 2026-11-07, every fleet session's build on the laptop went to the GPU host first (load 52 on 20 cores) while the away laptop, which leases its own builds, sat at load 3.5 of 24; a host that leases its own builds now decides on its sessions' builds before offering them; north#740
#[test]
fn a_session_build_is_offered_away_before_deciding_only_on_a_host_that_does_not_lease_its_own_builds() {
    let attended = laptop(|| None);
    let away = Host { lease_builds: || true, ..laptop(|| None) };
    assert!(offers_before_deciding(&attended, true), "the attended laptop sends its sessions' builds to a build host first");
    assert!(!offers_before_deciding(&away, true), "the away laptop decides on them here first");
    assert!(!offers_before_deciding(&attended, false), "a build outside a fleet session is decided here first");
}
