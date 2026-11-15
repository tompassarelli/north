use super::offering::{Declines, NO_BUILD_HOST, SESSION_BUILDS_OFF, after_offer, session_builds_off};
use super::request::Create;
use crate::capacity::build_offload::{Candidate, Declined, Job};
use crate::capacity::effects::Host;

fn never_pushed(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    unreachable!("the refusal decides nothing about pushing")
}

fn never_handed(_: &Candidate, _: &Job, _: std::os::unix::net::UnixStream, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    unreachable!("the refusal decides nothing about handing over")
}

fn laptop() -> Host {
    Host {
        serving: |_| None,
        warcraft_clients: || None,
        bar1_margin_mib: || None,
        gpu_slots: || None,
        offload: |_, _, _| 2,
        stage_runtime: |_, _, _| Ok(()),
        reenter_outside: |_| None,
        build_offload: never_pushed,
        hand_over: never_handed,
        lease_builds: || false,
        build_host: || false,
        session_builds: || true,
        reclaim_disk: || None,
        tend_disk: || {},
        keep_disk: || {},
        tend_slots: || {},
    }
}

fn every_host_full(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    Err(Some(Declined::NoRoom(vec!["gex45: 2.0 cores free of 20, needs 6".to_owned(), "cpx62: closed to offloaded builds (offload_builds = false)".to_owned(), "cpx62b: closed to offloaded builds (offload_builds = false)".to_owned()])))
}

fn every_host_closed(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    Err(Some(Declined::NoRoom(vec!["cpx62: closed to offloaded builds (offload_builds = false)".to_owned()])))
}

fn none_reachable(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    Err(Some(Declined::Unreachable(vec!["gex45: unreachable or unreadable over ssh".to_owned()])))
}

fn offload_failed(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    Err(Some(Declined::Failed("gex45: the lane did not sync".to_owned())))
}

fn no_build_host_reached(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    Err(None)
}

#[tokio::test(start_paused = true)]
async fn a_session_build_no_build_host_took_on_the_laptop_is_refused_naming_each_host_even_with_room_and_a_lease_here() {
    let create = Create { kind: "run", owner: "worker".to_owned(), timeout_seconds: Some(60) };
    let cargo = ["cargo".to_owned(), "test".to_owned()];
    let request = super::request::Request { name: "heavy".to_owned(), ..super::request::Request::default() };
    let declines = async |offloader: crate::capacity::build_offload::Offloader| {
        let host = Host { build_offload: offloader, lease_builds: || true, ..laptop() };
        let mut declined = Declines::new();
        let waiting = super::offering::Waiting { root: std::path::Path::new("/nonexistent"), ticket: None, request: &request, create: &create, command: &cargo, began: super::machine::now_ms(), declined: &mut declined };
        assert_eq!(super::offering::try_offload(waiting, false, &host, &mut super::terminations::Terminations::none(), None).await, None, "no build host took it");
        after_offer(&host, &cargo, &declined, &create)
    };
    for (offloader, named) in [(every_host_full as crate::capacity::build_offload::Offloader, "gex45: 2.0 cores free"), (every_host_closed, "cpx62: closed"), (none_reachable, "gex45: unreachable"), (offload_failed, "gex45: the lane did not sync")] {
        let refused = declines(offloader).await.expect("the incident of 2026-11-14: the build ran on the laptop at load 38");
        assert_eq!(refused.get("reason").and_then(serde_json::Value::as_str), Some(NO_BUILD_HOST));
        let detail = refused.get("detail").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
        assert!(detail.contains(named), "{detail}");
    }
    assert!(declines(no_build_host_reached).await.is_none(), "a host that reaches no build host at all decides on it itself");
    let off = Host { session_builds: || false, ..laptop() };
    let refused = session_builds_off(&off, &cargo, &create).expect("session_builds = false refuses the compile here");
    assert_eq!(refused.get("reason").and_then(serde_json::Value::as_str), Some(SESSION_BUILDS_OFF));
    let key = format!("[hosts.{}] session_builds = false, or offload_builds = false on a host that declares runners,", crate::platform::host::name());
    assert_eq!(refused.get("detail").and_then(serde_json::Value::as_str), Some(key.as_str()));
    assert!(session_builds_off(&laptop(), &cargo, &create).is_none(), "key absent or true: the compile runs here");
}
