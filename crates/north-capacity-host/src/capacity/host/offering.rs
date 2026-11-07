use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::decision::decision;
use super::handover::{self, Listening};
use super::json::Obj;
use super::machine::{env, now_ms};
use super::print_to;
use super::request::{Create, Request};
use super::state::dequeue;
use super::terminations::Terminations;
use crate::capacity::build_offload::{self, Candidate, Declined, Job};
use crate::capacity::effects::Host;

const OFFLOAD_DEFAULT_TIMEOUT_SECONDS: u64 = 3600;
pub(super) const NO_BUILD_HOST: &str = "NO_BUILD_HOST";
const OFFLOAD_RETRY_MILLISECONDS: f64 = 15_000.0;

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
    pub(super) retry_milliseconds: f64,
}

impl Declines {
    pub(super) fn new() -> Self {
        Self { printed: None, missing_host: None, retry_milliseconds: OFFLOAD_RETRY_MILLISECONDS }
    }

    /// The OFFLOAD_DECLINED line a decline earns, None when it repeats the reason already printed.
    pub(super) fn note(&mut self, why: Option<Declined>, owner: &str) -> Option<Obj> {
        let why = why?;
        self.missing_host = why.lacks_host().then(|| why.clone());
        self.retry_milliseconds = why.retry_milliseconds(OFFLOAD_RETRY_MILLISECONDS);
        why.changes(&mut self.printed).then(|| Obj::new().put("decision", "OFFLOAD_DECLINED").put("reason", why.reason()).put("detail", why.detail()).put("owner", owner))
    }
}

fn report_declined(why: Option<Declined>, create: &Create, declines: &mut Declines) {
    if let Some(line) = declines.note(why, create.owner.as_str()) {
        print_to(&line, true);
    }
}

/// A heavy build this host defers runs on another host with room instead of queueing here; None leaves it on the local path, printing why once per change of reason.
pub(super) async fn try_offload(waiting: Waiting<'_>, deferred: bool, host: &Host, terminations: &mut Terminations, taker: Option<std::os::unix::net::UnixStream>) -> Option<u8> {
    let Waiting { root, ticket, request, create, command, began, declined } = waiting;
    let patience = queue_time_left(create.timeout_seconds, (now_ms() - began) / 1000.0);
    let started = Arc::new(AtomicBool::new(false));
    let announced = Arc::clone(&started);
    let host = *host;
    let (root, ticket) = (root.to_path_buf(), ticket.map(str::to_owned));
    let (waiting_root, waiting_ticket) = (root.clone(), ticket.clone());
    let handed = taker.is_some();
    let landing = request.landing || env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty());
    let (class, owner, command) = (request.name.clone(), create.owner.clone(), build_offload::for_build_host(command));
    let timeout_seconds = create.timeout_seconds.unwrap_or(OFFLOAD_DEFAULT_TIMEOUT_SECONDS);
    let need = env("AGENT_CAPACITY_NEED").filter(|need| !need.is_empty());
    let agent = agent_session();
    let offloaded_already = env(build_offload::REMOTE_MARK).is_some();
    let artifact = crate::capacity::gate::builds_artifact(&command);
    let work = tokio::task::spawn_blocking(move || {
        let candidate = Candidate { class: &class, landing, need: need.as_deref(), deferred, agent, offloaded_already, artifact };
        let announce = |target: &str| {
            announced.store(true, Ordering::SeqCst);
            print_to(&Obj::new().put("decision", "OFFLOADED").put("reason", build_offload::REASON).put("host", target).put("class", class.as_str()).put("owner", owner.as_str()), true);
            match &ticket {
                Some(ticket) if handed => handover::set_aside(&root, ticket),
                Some(ticket) => dequeue(&root, ticket),
                None => {}
            }
        };
        let job = Job { owner: &owner, timeout_seconds, command: &command };
        match taker {
            Some(taker) => (host.hand_over)(&candidate, &job, taker, &announce),
            None => (host.build_offload)(&candidate, &job, &announce),
        }
    });
    let mut work = work;
    let mut stalled = std::pin::pin!(tokio::time::sleep(patience));
    let mut expired = false;
    let finished = loop {
        tokio::select! {
            done = &mut work => break Some(done),
            () = &mut stalled, if !expired => {
                if !started.load(Ordering::SeqCst) {
                    break None;
                }
                expired = true;
            }
            number = terminations.next() => {
                if let Some(ticket) = &waiting_ticket {
                    dequeue(&waiting_root, ticket);
                    handover::forget(&waiting_root, ticket);
                }
                std::process::exit(128 + number)
            }
        }
    };
    let Some(done) = finished else {
        if handed && let Some(ticket) = &waiting_ticket {
            handover::restore(&waiting_root, ticket);
        }
        return None;
    };
    match done.ok()? {
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

/// The decision that ends a fleet session's compile on a host that does not compile for sessions, once no build host took it and this host has no room beyond its reserves either; with room, the compile waits in this host's own heavy admission instead.
fn no_build_host(missing: &Declined, here: &str, create: &Create) -> Obj {
    Obj::new().put("decision", "REFUSED").put("reason", NO_BUILD_HOST).put("detail", format!("{}: {}; this host: {here}", missing.reason(), missing.detail())).put("owner", create.owner.as_str())
}

/// The refusal a fleet session's Rust compile gets on a host that does not compile for sessions: only once every build host declined it for want of a host and this host has no room beyond its reserves either.
pub(super) fn session_build_refusal(host: &Host, command: &[String], missing: Option<&Declined>, create: &Create) -> Option<Obj> {
    if (host.lease_builds)() || !crate::capacity::gate::compiles_rust(command) {
        return None;
    }
    let missing = missing?;
    let here = (host.own_build_refusal)()?;
    Some(no_build_host(missing, &here, create))
}

/// The refusal a fleet session's Rust compile gets when this host took it for want of a build host but its admission defers it: it waits in no queue, so no caller's deadline runs out behind a wait it never asked for, and is refused at once as when this host has no room.
pub(super) fn deferred_session_build(host: &Host, command: &[String], missing: Option<&Declined>, create: &Create, first: &Obj) -> Option<Obj> {
    if (host.lease_builds)() || !crate::capacity::gate::compiles_rust(command) || first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return None;
    }
    let reason = first.get("reason").and_then(Value::as_str).unwrap_or("DEFER");
    Some(no_build_host(missing?, &format!("its admission defers it ({reason})"), create))
}

/// Whether a heavy request goes to the build hosts before this host decides on it: a fleet session's, on a host that does not lease its own builds (the laptop under a playtest lease), so a build this host would admit still runs where there is room. A host that leases its own builds (a build host, the laptop with no playtest lease) decides first and offers the request elsewhere only when it defers it.
pub(super) fn offers_before_deciding(host: &Host, agent: bool) -> bool {
    agent && !(host.lease_builds)()
}

/// A request offered before deciding runs on a build host with room; any other runs elsewhere only when this host defers it on the first look. None sends it to the queue.
pub(super) async fn offer_first(mut waiting: Waiting<'_>, host: &Host, terminations: &mut Terminations) -> Option<Admitted> {
    let (root, request, create) = (waiting.root, waiting.request, waiting.create);
    let offered_before = offers_before_deciding(host, agent_session());
    if offered_before {
        if let Some(code) = try_offload(waiting.again(), false, host, terminations, None).await {
            return Some(Admitted::Offloaded(code));
        }
        if let Some(refused) = session_build_refusal(host, waiting.command, waiting.declined.missing_host.as_ref(), create) {
            return Some(Admitted::Here(refused));
        }
    }
    let first = decision(root, request, Some(create), None, host);
    if first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return Some(Admitted::Here(first));
    }
    if offered_before {
        return deferred_session_build(host, waiting.command, waiting.declined.missing_host.as_ref(), create, &first).map(Admitted::Here);
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
