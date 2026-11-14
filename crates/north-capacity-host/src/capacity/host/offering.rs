use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::decision::decision;
use super::handover::{self, Listening};
use super::json::Obj;
use super::offer_lifecycle::{self, Start};
use super::machine::{env, now_ms};
use super::print_to;
use super::request::{Create, Request};
use super::state::dequeue;
use super::terminations::Terminations;
use crate::capacity::build_offload::{self, Candidate, Declined, Job};
use crate::capacity::effects::Host;
use crate::foundation::clock::Timestamp;

const OFFLOAD_DEFAULT_TIMEOUT_SECONDS: u64 = 3600;
pub(super) const NO_BUILD_HOST: &str = "NO_BUILD_HOST";
pub(super) const HELD_HOST: &str = "HELD_HOST";
pub(super) const SESSION_BUILDS_OFF: &str = "SESSION_BUILDS_OFF";
const OFFLOAD_RETRY_MILLISECONDS: f64 = 15_000.0;
const OFFER_PENDING: u8 = 0;
const OFFER_STARTED: u8 = 1;
const OFFER_ABANDONED: u8 = 2;

pub(super) enum Admitted {
    Here(Obj),
    Offloaded(u8),
}

pub(super) struct Waiting<'a> {
    pub(super) root: &'a Path,
    pub(super) ticket: Option<&'a str>,
    pub(super) request: &'a Request,
    pub(super) create: &'a Create,
    pub(super) command: &'a [String],
    /// When the request began waiting, in milliseconds, so an offer that has not started never outlasts its --timeout-seconds.
    pub(super) began: f64,
    pub(super) declined: &'a mut Declines,
}

impl Waiting<'_> {
    pub(super) fn again(&mut self) -> Waiting<'_> {
        Waiting { root: self.root, ticket: self.ticket, request: self.request, create: self.create, command: self.command, began: self.began, declined: &mut *self.declined }
    }
}

/// How long a queued request may still wait before its --timeout-seconds ends it; without one it waits on.
pub(super) fn queue_time_left(timeout_seconds: Option<u64>, queued_seconds: f64) -> Duration {
    timeout_seconds.map_or(Duration::MAX, |timeout| Duration::from_secs_f64((timeout as f64 - queued_seconds).max(0.0)))
}

/// What the offload offers so far have declined for: the reason last printed, which an unchanged decline stays silent about, and how long the next offer waits, stretched while every offload host is out of reach so a queued build stops probing hosts this host has no path to.
pub(super) struct Declines {
    printed: Option<String>,
    missing_host: Option<Declined>,
    pub(super) kept_here: Option<Declined>,
    pub(super) retry_milliseconds: f64,
}

impl Declines {
    pub(super) fn new() -> Self {
        Self { printed: None, missing_host: None, kept_here: None, retry_milliseconds: OFFLOAD_RETRY_MILLISECONDS }
    }

    /// The OFFLOAD_DECLINED line a decline earns, None when it repeats the reason already printed.
    pub(super) fn note(&mut self, why: Option<Declined>, owner: &str) -> Option<Obj> {
        let Some(why) = why else {
            (self.missing_host, self.kept_here) = (None, None);
            return None;
        };
        self.missing_host = why.lacks_host().then(|| why.clone());
        self.kept_here = (!why.lacks_host()).then(|| why.clone());
        self.retry_milliseconds = why.retry_milliseconds(OFFLOAD_RETRY_MILLISECONDS);
        why.changes(&mut self.printed).then(|| Obj::new().put("decision", "OFFLOAD_DECLINED").put("reason", why.reason()).put("detail", why.detail()).put("owner", owner))
    }
}

fn report_declined(why: Option<Declined>, create: &Create, declines: &mut Declines) {
    if let Some(line) = declines.note(why, create.owner.as_str()) {
        print_to(&line, true);
    }
}

async fn await_offer(mut work: tokio::task::JoinHandle<Result<u8, Option<Declined>>>, patience: Duration, (offer, offer_id): (&AtomicU8, &str), terminations: &mut Terminations, (root, ticket): (&Path, Option<&str>)) -> Option<Result<Result<u8, Option<Declined>>, tokio::task::JoinError>> {
    let mut stalled = std::pin::pin!(tokio::time::sleep(patience));
    let mut expired = false;
    let finished = loop {
        tokio::select! {
            done = &mut work => break Some(done),
            () = &mut stalled, if !expired => {
                if offer.compare_exchange(OFFER_PENDING, OFFER_ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    break None;
                }
                expired = true;
            }
            number = terminations.next() => {
                if offer.load(Ordering::SeqCst) != OFFER_STARTED {
                    offer_lifecycle::abandoned(root, offer_id, &format!("its gate was stopped by signal {number}"));
                }
                if let Some(ticket) = ticket {
                    dequeue(root, ticket);
                    handover::forget(root, ticket);
                }
                std::process::exit(128 + number)
            }
        }
    };
    if offer.load(Ordering::SeqCst) != OFFER_STARTED {
        let cause = if finished.is_some() { "no build host took it" } else { "the gate's timeout came before any build host announced it" };
        offer_lifecycle::abandoned(root, offer_id, cause);
    }
    finished
}

/// A heavy build this host defers runs on another host with room instead of queueing here; None leaves it on the local path, printing why once per change of reason.
pub(super) async fn try_offload(waiting: Waiting<'_>, deferred: bool, host: &Host, terminations: &mut Terminations, taker: Option<std::os::unix::net::UnixStream>) -> Option<u8> {
    let Waiting { root, ticket, request, create, command, began, declined } = waiting;
    let patience = queue_time_left(create.timeout_seconds, (now_ms() - began) / 1000.0);
    let offer_id = format!("{} {} {}", create.owner, std::process::id(), Timestamp::now().unix_millis());
    offer_lifecycle::offered(root, &offer_id, patience);
    let started_id = offer_id.clone();
    let offer = Arc::new(AtomicU8::new(OFFER_PENDING));
    let announced = Arc::clone(&offer);
    let host = *host;
    let (root, ticket) = (root.to_path_buf(), ticket.map(str::to_owned));
    let (waiting_root, waiting_ticket) = (root.clone(), ticket.clone());
    let handed = taker.is_some();
    let landing = request.landing || env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty());
    let (class, owner, command) = (request.name.clone(), create.owner.clone(), crate::capacity::build_host_command::for_build_host(command, landing));
    let timeout_seconds = create.timeout_seconds.unwrap_or(OFFLOAD_DEFAULT_TIMEOUT_SECONDS);
    let cpus = request.resources().cpus;
    let need = env("AGENT_CAPACITY_NEED").filter(|need| !need.is_empty());
    let agent = agent_session();
    let offloaded_already = env(build_offload::REMOTE_MARK).is_some();
    let artifact = crate::capacity::gate::builds_artifact(&command);
    let work = tokio::task::spawn_blocking(move || {
        let candidate = Candidate { class: &class, landing, need: need.as_deref(), deferred, agent, offloaded_already, artifact };
        let announce = |target: &str| {
            if announced.compare_exchange(OFFER_PENDING, OFFER_STARTED, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                return false;
            }
            print_to(&Obj::new().put("decision", "OFFLOADED").put("reason", build_offload::REASON).put("host", target).put("class", class.as_str()).put("owner", owner.as_str()), true);
            offer_lifecycle::started(&root, &started_id, target, &Start { ticket: ticket.as_deref(), handed }, |start| match start.ticket {
                Some(ticket) if start.handed => handover::set_aside(&root, ticket),
                Some(ticket) => dequeue(&root, ticket),
                None => {}
            });
            true
        };
        let job = Job { owner: &owner, timeout_seconds, command: &command, cpus };
        match taker {
            Some(taker) => (host.hand_over)(&candidate, &job, taker, &announce),
            None => (host.build_offload)(&candidate, &job, &announce),
        }
    });
    let finished = await_offer(work, patience, (&offer, &offer_id), terminations, (&waiting_root, waiting_ticket.as_deref())).await;
    let failed = |why: &str| Err(Some(Declined::Failed(why.to_owned())));
    let outcome = match finished {
        Some(Ok(done)) => done,
        Some(Err(_)) => failed("the offload stopped before it reported"),
        None => failed("no build host started it before its --timeout-seconds"),
    };
    match outcome {
        Ok(code) => Some(code),
        Err(why) => {
            if handed && let Some(ticket) = &waiting_ticket {
                handover::restore(&waiting_root, ticket);
            }
            report_declined(why, create, declined);
            None
        }
    }
}

/// The refusal a fleet session's build gets on a host that is not a build host once every build host was full, closed or out of reach or the offload failed: it never runs here, whatever this host's own room, lease or reserve, and the detail names each host and why.
fn no_build_host(missing: &Declined, create: &Create) -> Obj {
    Obj::new().put("decision", "REFUSED").put("reason", NO_BUILD_HOST).put("detail", format!("{}: {}", missing.reason(), missing.detail())).put("owner", create.owner.as_str())
}

/// The refusal a fleet session's Rust compile gets on a host whose operator entry sets `session_builds = false`: once no build host took it, whatever this host's room or lease.
pub(super) fn session_builds_off(host: &Host, command: &[String], create: &Create) -> Option<Obj> {
    if (host.session_builds)() || !crate::capacity::gate::compiles_rust(command) {
        return None;
    }
    let key = format!("[hosts.{}] session_builds = false", crate::platform::host::name());
    Some(Obj::new().put("decision", "REFUSED").put("reason", SESSION_BUILDS_OFF).put("detail", key).put("owner", create.owner.as_str()))
}

/// The refusal a fleet session's Rust compile gets on a host whose hold keeps heavy work off it when the build hosts declined it for a reason that leaves it on this host (it builds an artifact the caller runs, ships too large a diff): the hold binds that fallback too, so the build never runs here.
pub(super) fn held_build_refusal(host: &Host, command: &[String], kept: Option<&Declined>, create: &Create) -> Option<Obj> {
    if (host.lease_builds)() || !crate::capacity::gate::compiles_rust(command) {
        return None;
    }
    let kept = kept?;
    Some(Obj::new().put("decision", "REFUSED").put("reason", HELD_HOST).put("detail", format!("{}: {}", kept.reason(), kept.detail())).put("owner", create.owner.as_str()))
}

/// The refusal a run offered before deciding gets once no build host took it, None when this host's own admission decides on it: only a decline that keeps the build here for a reason of its own, or a host that reaches no build host at all, leaves it that.
pub(super) fn after_offer(host: &Host, command: &[String], declined: &Declines, create: &Create) -> Option<Obj> {
    session_builds_off(host, command, create).or_else(|| declined.missing_host.as_ref().map(|missing| no_build_host(missing, create))).or_else(|| held_build_refusal(host, command, declined.kept_here.as_ref(), create))
}

/// A request offered before deciding runs on a build host with room or is refused; any other runs elsewhere only when this host defers it on the first look. None sends it to the queue.
pub(super) async fn offer_first(mut waiting: Waiting<'_>, host: &Host, terminations: &mut Terminations) -> Option<Admitted> {
    let (root, request, create) = (waiting.root, waiting.request, waiting.create);
    let offered_before = build_offload::offers_first(agent_session(), (host.build_host)());
    if offered_before {
        if let Some(code) = try_offload(waiting.again(), false, host, terminations, None).await {
            return Some(Admitted::Offloaded(code));
        }
        if let Some(refused) = after_offer(host, waiting.command, waiting.declined, create) {
            return Some(Admitted::Here(refused));
        }
    }
    let first = decision(root, request, Some(create), None, host);
    if first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return Some(Admitted::Here(first));
    }
    if offered_before {
        return None;
    }
    try_offload(waiting, true, host, terminations, None).await.map(Admitted::Offloaded)
}

/// The next peer that opened this run's door, never when it has none.
pub(super) async fn accepted(handover: Option<&Listening>) -> std::os::unix::net::UnixStream {
    match handover {
        Some(handover) => match handover.accept().await {
            Some(stream) => stream,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

/// Whether this build was started by a fleet session that has no local need: its compiles belong on a build host before this one.
pub(super) fn agent_session() -> bool {
    build_offload::is_agent_build(env)
}
