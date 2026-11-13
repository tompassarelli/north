//! Waiting for a decision: the ticket a request gets, what a waiter watches, and the admission loop.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::super::decision::decision;
use super::super::handover::Listening;
use super::super::json::{Obj, num, round2};
use super::super::machine::{enclosing_lease, env, now_ms};
use super::super::state::read_leases;
use super::super::offering::{Admitted, Declines, Waiting, accepted, agent_session, offer_first, queue_time_left, try_offload};
use super::super::policy::batch_class;
use super::super::quiet_holds::QuietHolds;
use super::super::request::{Create, Request, class, id, is_run};
use super::super::requeue;
use super::super::state::{dequeue, enqueue, with_lock};
use super::super::terminations::Terminations;
use super::super::print_to;
use crate::capacity::effects::Host;

const QUEUE_POLL_MILLISECONDS: u64 = 1000;
const BEHIND_POLL_MILLISECONDS: u64 = 10_000;

fn modified(directory: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(directory).and_then(|metadata| metadata.modified()).ok()
}

/// The lease files present, by name: each running lease's usage sample is rewritten beside them about twice a second, so the directory's modification time says nothing about a lease appearing or going.
fn lease_names(root: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(root.join("leases"))
        .map(|entries| entries.flatten().map(|entry| entry.file_name()).filter(|name| Path::new(name).extension().is_some_and(|extension| extension == "json")).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// How a waiting ticket watches for the next decision: one held behind an earlier ticket (DEFER_QUEUED) can only move once a lease or a ticket appears, goes or records a new reason, so it decides again on such a change or every 10 s; any other waits for a lease to appear or go or the 1 s poll that reads falling pressure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Watch {
    pub poll: Duration,
    pub queue: bool,
}

pub(super) fn watch(reason: Option<&str>) -> Watch {
    if reason == Some("DEFER_QUEUED") {
        Watch { poll: Duration::from_millis(BEHIND_POLL_MILLISECONDS), queue: true }
    } else {
        Watch { poll: Duration::from_millis(QUEUE_POLL_MILLISECONDS), queue: false }
    }
}

/// What a waiter compares to see a decision-relevant change: the lease file names, plus the queue directory's modification time for a ticket held behind an earlier one.
pub(super) fn watched_state(root: &Path, watch: Watch) -> (Vec<std::ffi::OsString>, Option<std::time::SystemTime>) {
    (lease_names(root), watch.queue.then(|| modified(&root.join("queue"))).flatten())
}

/// Waits until the watched state changes or the jittered poll interval passes.
pub(super) async fn state_change(root: &Path, watch: Watch) {
    let jitter = u64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos())) % QUEUE_POLL_MILLISECONDS;
    let deadline = tokio::time::Instant::now() + watch.poll + Duration::from_millis(jitter);
    let before = watched_state(root, watch);
    while tokio::time::Instant::now() < deadline && watched_state(root, watch) == before {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn reason(result: &Obj) -> Option<String> {
    result.get("reason").and_then(Value::as_str).map(str::to_owned)
}

fn wait_ticket(root: &Path, request: &Request, create: &Create, pullable: bool) -> String {
    enqueue(
        root,
        Obj::new()
            .put("class", request.admitted_class())
            .put("requestedClass", request.name.as_str())
            .put("requestedCpus", num(request.resources().cpus))
            .put("requestedMemoryMiB", num(request.resources().memory_mib))
            .put("owner", create.owner.as_str())
            .put("timeoutSeconds", create.timeout_seconds)
            .put("newClients", request.new_clients)
            .put("newDesktops", request.new_desktops)
            .put_some("alone", request.alone.then_some(true))
            .put_some("priority", request.priority)
            .put_some("pullable", pullable.then_some(true)),
    )
}

/// Batch work, and a native client or desktop start given --wait-seconds, is held in arrival order until admitted, its --wait-seconds pass (ending in its last DEFER), or one quiet native session alone holds it past its timeout (REFUSED).
pub(super) async fn admit(root: &Path, request: &Request, create: &Create, command: &[String], host: &Host, terminations: &mut Terminations) -> Admitted {
    if !batch_class(&request.name) && request.wait_seconds.is_none() && !request.alone {
        return Admitted::Here(decision(root, request, Some(create), None, host));
    }
    let offer = (request.name == "heavy" || request.name == "moderate" && crate::capacity::gate::compiles_rust(command)) && !request.landing;
    let mut declined = Declines::new();
    let began = now_ms();
    let resumed = requeue::resumed(root);
    if resumed.is_none() && offer && let Some(admitted) = offer_first(Waiting { root, ticket: None, request, create, command, began, declined: &mut declined }, host, terminations).await {
        return admitted;
    }
    let pullable = offer && agent_session() && request.name == "heavy" && crate::capacity::handover::pullable(command);
    let mut offered_at = now_ms();
    let follow = requeue::Follow::here();
    let (ticket, queued_at) = resumed.map_or_else(|| (wait_ticket(root, request, create, pullable), began), |resumed| (resumed.ticket, resumed.queued_at));
    let handover = pullable.then(|| Listening::open(root, &ticket)).flatten();
    let mut reported = None;
    let mut quiet = QuietHolds::default();
    loop {
        if let Some(door) = &handover {
            door.tend();
        }
        let result = decision(root, request, Some(create), Some(&ticket), host);
        let queued_seconds = round2((now_ms() - queued_at) / 1000.0);
        let result = result.put("queuedSeconds", num(queued_seconds));
        let decided = result.get("decision").and_then(Value::as_str).unwrap_or_default();
        if decided == "RESERVED" || decided == "REFUSED" || request.wait_seconds.is_some_and(|wait| queued_seconds >= wait as f64) {
            dequeue(root, &ticket);
            return Admitted::Here(result);
        }
        if quiet.outlasts(&result.value(), now_ms(), create.timeout_seconds) {
            dequeue(root, &ticket);
            return Admitted::Here(result.put("decision", "REFUSED"));
        }
        if create.timeout_seconds.is_some_and(|timeout| queued_seconds >= timeout as f64) {
            dequeue(root, &ticket);
            let ended = if reason(&result).as_deref() == Some("DEFER_NATIVE_QUIET") { "REFUSED" } else { "TIMED_OUT" };
            return Admitted::Here(result.put("decision", ended));
        }
        if offer && now_ms() - offered_at >= declined.retry_milliseconds {
            offered_at = now_ms();
            if let Some(code) = try_offload(Waiting { root, ticket: Some(&ticket), request, create, command, began: queued_at, declined: &mut declined }, reason(&result).is_some_and(|reason| reason.starts_with("DEFER_")), host, terminations, None).await {
                dequeue(root, &ticket);
                return Admitted::Offloaded(code);
            }
        }
        if reason(&result) != reported {
            print_to(&result.clone().put("decision", "QUEUED"), true);
            reported = reason(&result);
        }
        if let Some(follow) = &follow {
            follow.follow(&ticket, queued_at);
        }
        tokio::select! {
            () = state_change(root, watch(reason(&result).as_deref())) => {}
            () = tokio::time::sleep(queue_time_left(create.timeout_seconds, queued_seconds)) => {}
            taker = accepted(handover.as_ref()) => {
                let on_deferred = reason(&result).is_some_and(|reason| reason.starts_with("DEFER_"));
                if let Some(code) = try_offload(Waiting { root, ticket: Some(&ticket), request, create, command, began: queued_at, declined: &mut declined }, on_deferred, host, terminations, Some(taker)).await {
                    dequeue(root, &ticket);
                    return Admitted::Offloaded(code);
                }
                if let Some(door) = &handover {
                    door.give_back();
                }
            }
            number = terminations.next() => {
                dequeue(root, &ticket);
                super::super::handover::forget(root, &ticket);
                std::process::exit(128 + number);
            }
        }
    }
}

/// The batch lease this process already runs inside, by its environment or its cgroup scope.
pub(super) fn nested_parent(root: &Path) -> Option<String> {
    let ids = [env("AGENT_CAPACITY_LEASE"), enclosing_lease()];
    with_lock(root, || {
        let (active, _) = read_leases(root, now_ms());
        active
            .iter()
            .find(|lease| is_run(lease) && batch_class(class(lease)) && ids.iter().flatten().any(|candidate| candidate == id(lease)))
            .map(|lease| id(lease).to_owned())
    })
}
