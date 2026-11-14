use super::offering::{Declines, HELD_HOST, NO_BUILD_HOST, SESSION_BUILDS_OFF, after_offer, held_build_refusal, session_builds_off};
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
    let key = format!("[hosts.{}] session_builds = false", crate::platform::host::name());
    assert_eq!(refused.get("detail").and_then(serde_json::Value::as_str), Some(key.as_str()));
    assert!(session_builds_off(&laptop(), &cargo, &create).is_none(), "key absent or true: the compile runs here");
}

static RELEASED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn stalled(_: &Candidate, _: &Job, _: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    north_base::platform::wait::until(std::time::Duration::from_secs(300), || RELEASED.load(std::sync::atomic::Ordering::SeqCst));
    Err(None)
}

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PAST_TIMEOUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn running_until_released(_: &Candidate, _: &Job, announce: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    announce("gex45");
    STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
    north_base::platform::wait::until(std::time::Duration::from_secs(300), || PAST_TIMEOUT.load(std::sync::atomic::Ordering::SeqCst));
    Ok(7)
}

async fn offer(host: &Host, queued_seconds: u64) -> Option<u8> {
    let request = super::request::Request { name: "heavy".to_owned(), ..super::request::Request::default() };
    let create = Create { kind: "run", owner: "worker".to_owned(), timeout_seconds: Some(5) };
    let command = ["cargo".to_owned(), "test".to_owned()];
    let mut declined = super::offering::Declines::new();
    let began = super::machine::now_ms() - queued_seconds as f64 * 1000.0;
    let waiting = super::offering::Waiting { root: std::path::Path::new("/nonexistent"), ticket: None, request: &request, create: &create, command: &command, began, declined: &mut declined };
    super::offering::try_offload(waiting, true, host, &mut super::terminations::Terminations::none(), None).await
}

#[tokio::test(start_paused = true)]
async fn an_offload_offer_that_never_starts_ends_with_the_gates_timeout_and_one_already_running_does_not() {
    let hung = Host { build_offload: stalled, ..laptop() };
    let ended = tokio::time::timeout(std::time::Duration::from_secs(60), offer(&hung, 5)).await.expect("the stalled offer outlived the gate's timeout");
    assert_eq!(ended, None, "the stalled offer ended with the gate's timeout");
    RELEASED.store(true, std::sync::atomic::Ordering::SeqCst);
    let running = Host { build_offload: running_until_released, ..laptop() };
    let release_once_past_the_timeout = async {
        while !STARTED.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        PAST_TIMEOUT.store(true, std::sync::atomic::Ordering::SeqCst);
    };
    let (finished, ()) = tokio::join!(offer(&running, 4), release_once_past_the_timeout);
    assert_eq!(finished, Some(7), "a job a host started finishes past the gate's timeout");
}

static GAVE_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static HOST_RUNS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ANNOUNCE_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn announcing_after_the_gate_gave_up(_: &Candidate, _: &Job, announce: &dyn Fn(&str) -> bool) -> Result<u8, Option<Declined>> {
    north_base::platform::wait::until(std::time::Duration::from_secs(300), || GAVE_UP.load(std::sync::atomic::Ordering::SeqCst));
    if announce("gex45") {
        HOST_RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    ANNOUNCE_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    Err(None)
}

#[tokio::test(start_paused = true)]
async fn an_offer_whose_timeout_fires_at_announce_runs_the_job_once_here_and_never_on_the_host() {
    let racing = Host { build_offload: announcing_after_the_gate_gave_up, ..laptop() };
    assert_eq!(offer(&racing, 5).await, None, "the gate gave up, so the job runs on the local path");
    GAVE_UP.store(true, std::sync::atomic::Ordering::SeqCst);
    north_base::platform::wait::until(std::time::Duration::from_secs(30), || ANNOUNCE_DONE.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(HOST_RUNS.load(std::sync::atomic::Ordering::SeqCst), 0, "the host must not also start the abandoned offer");
}

#[test]
fn a_build_offload_declines_as_an_artifact_is_refused_under_a_hold_and_runs_here_only_without_one() {
    let create = Create { kind: "run", owner: "n892-testclock".to_owned(), timeout_seconds: Some(3600) };
    let build = ["cargo", "build", "--locked", "-p", "north", "--bin", "north"].map(str::to_owned);
    let mut declines = Declines::new();
    assert!(declines.note(Some(Declined::artifact()), "n892-testclock").is_some());
    let playing = laptop();
    let refused = held_build_refusal(&playing, &build, declines.kept_here.as_ref(), &create).expect("the hold keeps the build off this host");
    assert_eq!(refused.get("decision").and_then(serde_json::Value::as_str), Some("REFUSED"));
    assert_eq!(refused.get("reason").and_then(serde_json::Value::as_str), Some(HELD_HOST));
    assert!(refused.get("detail").and_then(serde_json::Value::as_str).unwrap_or_default().contains("artifact"));
    let free = Host { lease_builds: || true, ..laptop() };
    assert!(held_build_refusal(&free, &build, declines.kept_here.as_ref(), &create).is_none(), "no hold: the build runs here as before");
    assert!(held_build_refusal(&playing, &build, Declines::new().kept_here.as_ref(), &create).is_none(), "nothing declined: nothing to hold");
    let mut full = Declines::new();
    full.note(Some(Declined::NoRoom(vec!["gex45: busy".to_owned()])), "n892-testclock");
    assert!(held_build_refusal(&playing, &build, full.kept_here.as_ref(), &create).is_none(), "a want of a build host is the session-build refusal's, not this one's");
}
