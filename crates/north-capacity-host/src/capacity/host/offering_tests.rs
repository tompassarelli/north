use super::json::Obj;
use super::offering::{deferred_session_build, session_build_refusal};
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
