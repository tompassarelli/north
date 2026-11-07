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
    let playing = laptop(|| None);
    let free = Host { lease_builds: || true, ..laptop(|| None) };
    assert!(offers_before_deciding(&playing, true), "the laptop under a playtest lease sends its sessions' builds to a build host first");
    assert!(!offers_before_deciding(&free, true), "the laptop with no playtest lease decides on them here first");
    assert!(!offers_before_deciding(&playing, false), "a build outside a fleet session is decided here first");
}

static RELEASED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn stalled(_: &Candidate, _: &Job, _: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    north_base::platform::wait::until(std::time::Duration::from_secs(300), || RELEASED.load(std::sync::atomic::Ordering::SeqCst));
    Err(None)
}

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PAST_TIMEOUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn running_until_released(_: &Candidate, _: &Job, announce: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
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

// KEEP(bug): on 2026-11-06 a queued gate with --timeout-seconds 3600 sat at DEFER_CPU_PRESSURE for 6,498 s because its admit loop was stuck awaiting an offload offer that never answered; an offer that has not started now gives up when the gate's timeout ends, while one a host has taken runs on; north#634
#[tokio::test(start_paused = true)]
async fn an_offload_offer_that_never_starts_ends_with_the_gates_timeout_and_one_already_running_does_not() {
    let hung = Host { build_offload: stalled, ..laptop(|| None) };
    let ended = tokio::time::timeout(std::time::Duration::from_secs(60), offer(&hung, 5)).await.expect("the stalled offer outlived the gate's timeout");
    assert_eq!(ended, None, "the stalled offer ended with the gate's timeout");
    RELEASED.store(true, std::sync::atomic::Ordering::SeqCst);
    let running = Host { build_offload: running_until_released, ..laptop(|| None) };
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
