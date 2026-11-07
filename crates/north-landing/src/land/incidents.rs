use std::path::Path;

use crate::platform::attribution::{self, Cause, Harness};
use crate::platform::push_ledger;
use super::checks::Verdict;
use super::queue::{self, Ticket};
use super::{records, telemetry};

pub const LANDER: &str = "lander";

pub struct Noticed<'a> {
    pub class: &'a str,
    pub key: String,
    pub noticed_by: &'a str,
    pub detail: &'a str,
}

pub fn class(verdict: &str) -> Option<&'static str> {
    if verdict.contains("deadline") || verdict.contains(" hung") {
        return Some("lander-hang");
    }
    if verdict.contains("persistent transient") || verdict.contains("persistent deferred") {
        return Some("lander-transient");
    }
    match verdict.split_whitespace().next() {
        Some("failed") => Some("lander-refused"),
        Some("conflict") => Some("lander-conflict"),
        Some("transient") => Some("lander-transient"),
        Some("deferred") => Some("lander-deferred"),
        Some("withdrawn") => Some("lander-withdrawn"),
        _ => None,
    }
}

/// The side a lander verdict belongs to: a failing check or a conflict in the lane's own lines is the worker's; a deferral, a hang, a transient error or an origin that refused the push is the harness's.
pub fn blame(verdict: &str) -> Option<Cause> {
    if verdict.starts_with("failed origin refused") {
        return Some(Cause::Harness(Harness::LandingRefusal));
    }
    match class(verdict)? {
        "lander-refused" | "lander-conflict" => Some(Cause::Worker),
        "lander-transient" | "lander-deferred" | "lander-hang" => Some(Cause::Harness(Harness::LandingRefusal)),
        _ => None,
    }
}

/// Records the cause of a refused or held lane for the fleet session that pushed it.
pub fn attribute(ticket: &Ticket, verdict: &str) {
    if let (Some(session), Some(cause)) = (ticket.session.as_deref(), blame(verdict)) {
        attribution::record(session, LANDER, cause, verdict);
    }
}

fn name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A ticket the lander decided other than landed, keyed by its ticket file so a replayed decision adds no row.
pub fn decided<'a>(path: &Path, ticket: &Ticket, verdict: &'a str) -> Option<Noticed<'a>> {
    let class = class(verdict)?;
    Some(Noticed { class, key: format!("{class}:{}@{}", name(path), ticket.head), noticed_by: LANDER, detail: verdict })
}

/// A check of `head` on `base` that ended without a verdict, recorded the moment it ends and naming its log, once per head and base however many rounds repeat it.
pub fn unsettled(queue: &Path, path: &Path, ticket: &Ticket, head: &str, base: &str, verdict: &Verdict, log: &Path) {
    let (kind @ ("transient" | "deferred"), detail) = verdict.class() else { return };
    let detail = format!("{kind} {detail}; log: {}", log.display());
    let Some(class) = class(&detail) else { return };
    record(queue, path, ticket, Some(Noticed { class, key: format!("{class}:{head}@{base}"), noticed_by: LANDER, detail: &detail }));
}

pub fn arguments(path: &Path, ticket: &Ticket, noticed: &Noticed, at: f64) -> Vec<String> {
    let mut arguments: Vec<String> = ["ledger", "incident", "record", "--key", &noticed.key, "--class", noticed.class, "--noticed-by", noticed.noticed_by].iter().map(|word| (*word).to_owned()).collect();
    let mut flag = |name: &str, value: String| {
        arguments.push(name.to_owned());
        arguments.push(value);
    };
    flag("--repo", push_ledger::repository(&ticket.origin));
    flag("--subject", ticket.branch.clone());
    let queued = queue::queued_micros(path);
    if queued != u128::MAX {
        flag("--started-at", (queued as f64 / 1e6).to_string());
    }
    flag("--detected-at", at.to_string());
    flag("--detail", noticed.detail.to_owned());
    arguments
}

/// Records the incident through `north ledger incident record` in the background, as the landings row is.
pub fn record(queue: &Path, path: &Path, ticket: &Ticket, noticed: Option<Noticed>) {
    let Some(noticed) = noticed else { return };
    if let Err(error) = records::spawn(queue, &arguments(path, ticket, &noticed, telemetry::epoch())) {
        println!("north land: could not record the {} incident for {}: {error}", noticed.class, ticket.branch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_refused_for_its_own_failing_check_is_recorded_worker_caused() {
        assert_eq!(blame("failed /tmp/land/check.log"), Some(Cause::Worker));
        assert_eq!(blame("transient capacity"), Some(Cause::Harness(Harness::LandingRefusal)));
        assert_eq!(blame("failed origin refused the push of abc: hook declined"), Some(Cause::Harness(Harness::LandingRefusal)));
    }
}
