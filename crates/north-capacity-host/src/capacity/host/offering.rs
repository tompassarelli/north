use std::path::Path;

use serde_json::Value;

use super::decision::decision;
use super::handover::Listening;
use super::json::Obj;
use super::machine::env;
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
    pub(super) declined: &'a mut Declines,
}

impl Waiting<'_> {
    pub(super) fn again(&mut self) -> Waiting<'_> {
        Waiting { root: self.root, ticket: self.ticket, request: self.request, create: self.create, command: self.command, declined: &mut *self.declined }
    }
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
    let Waiting { root, ticket, request, create, command, declined } = waiting;
    let host = *host;
    let (root, ticket) = (root.to_path_buf(), ticket.map(str::to_owned));
    let (waiting_root, waiting_ticket) = (root.clone(), ticket.clone());
    let landing = request.landing || env("SAFE_PUSH_LAND_HEAD").is_some_and(|head| !head.is_empty());
    let (class, owner, command) = (request.name.clone(), create.owner.clone(), build_offload::for_build_host(command));
    let timeout_seconds = create.timeout_seconds.unwrap_or(OFFLOAD_DEFAULT_TIMEOUT_SECONDS);
    let need = env("AGENT_CAPACITY_NEED").filter(|need| !need.is_empty());
    let agent = agent_session();
    let offloaded_already = env(build_offload::REMOTE_MARK).is_some();
    let work = tokio::task::spawn_blocking(move || {
        let candidate = Candidate { class: &class, landing, need: need.as_deref(), deferred, agent, offloaded_already };
        let announce = |target: &str| {
            print_to(&Obj::new().put("decision", "OFFLOADED").put("reason", build_offload::REASON).put("host", target).put("class", class.as_str()).put("owner", owner.as_str()), true);
            if let Some(ticket) = &ticket {
                dequeue(&root, ticket);
            }
        };
        let job = Job { owner: &owner, timeout_seconds, command: &command };
        match taker {
            Some(taker) => (host.hand_over)(&candidate, &job, taker, &announce),
            None => (host.build_offload)(&candidate, &job, &announce),
        }
    });
    tokio::select! {
        done = work => match done.ok()? {
            Ok(code) => Some(code),
            Err(why) => {
                report_declined(why, create, declined);
                None
            }
        },
        number = terminations.next() => {
            if let Some(ticket) = &waiting_ticket {
                dequeue(&waiting_root, ticket);
            }
            std::process::exit(128 + number)
        }
    }
}

/// The decision that ends a fleet session's compile on a host that does not compile for sessions, once no build host took it.
fn no_build_host(missing: &Declined, create: &Create) -> Obj {
    Obj::new().put("decision", "REFUSED").put("reason", NO_BUILD_HOST).put("detail", format!("{}: {}", missing.reason(), missing.detail())).put("owner", create.owner.as_str())
}

/// A fleet session's heavy request is offered to the build hosts before this host decides on it, so a build this host would admit still runs where there is room; any other heavy request runs elsewhere only when its host defers it on the first look. None sends it to the queue.
pub(super) async fn offer_first(mut waiting: Waiting<'_>, host: &Host, terminations: &mut Terminations) -> Option<Admitted> {
    let (root, request, create) = (waiting.root, waiting.request, waiting.create);
    if agent_session() {
        if let Some(code) = try_offload(waiting.again(), false, host, terminations, None).await {
            return Some(Admitted::Offloaded(code));
        }
        if !(host.lease_builds)() && crate::capacity::gate::compiles_rust(waiting.command) && let Some(missing) = waiting.declined.missing_host.as_ref() {
            return Some(Admitted::Here(no_build_host(missing, create)));
        }
    }
    let first = decision(root, request, Some(create), None, host);
    if first.get("decision").and_then(Value::as_str) != Some("DEFER") {
        return Some(Admitted::Here(first));
    }
    if agent_session() {
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
