//! The spin signals one box's run shows, each named so the escalation it causes records why it climbed: the
//! worker ran out of approach, the lander refused the same lane twice with the same verdict, a missed check
//! reopened the box, or the run took twice the ETA its brief stated. A run that shows one is no try toward
//! the two a day that hand a box to the proxy, and its box climbs one rung of `tier::Rung::LADDER`
//! (docs/rationale/nearest-routing.md section 5).
use super::stops;
use crate::ledger::{RunEnd, RunTime};
use crate::platform::land_notice::Notice;
use serde_json::Value;
use std::collections::BTreeMap;

/// The named signal that a box is spinning on its rung.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    /// The run ended not done and no boundary of the harness recorded a cause for it, so it is the worker's on its rung.
    Reasoned,
    /// The lander refused the same lane twice with the same verdict since it last landed.
    LandRefusal,
    /// A missed check reopened the box the run left open.
    Reopened,
    /// The run took twice the ETA its brief stated.
    EtaOverrun,
}

impl Signal {
    /// The word a launch record keeps for the signal that climbed it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Reasoned => "reasoning",
            Self::LandRefusal => "land-refusal",
            Self::Reopened => "reopened",
            Self::EtaOverrun => "eta-overrun",
        }
    }
}

/// The signal that fired and the evidence behind it, which the climb's reason and its brief carry.
#[derive(Clone, Debug, PartialEq)]
pub struct Spin {
    pub signal: Signal,
    pub evidence: String,
}

/// What the records hold about the run and the box it leaves open.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Evidence {
    /// Whether a missed check reopened the box the run was staffed on.
    pub reopened: bool,
    /// The run's wall minutes and stated ETA, as the ledger recorded them.
    pub time: Option<RunTime>,
    /// How the ledger settled the run's end, absent before the ledger holds it.
    pub end: Option<RunEnd>,
}

/// Why the session in `north fleet show NAME --json` shows a spin signal, with the evidence behind it.
pub fn observed(session: &Value, evidence: &Evidence) -> Option<Spin> {
    if session["entries"].as_array().is_some_and(|entries| entries.iter().any(|entry| entry["kind"] == "refusal")) {
        return None;
    }
    stops::final_report(session)?;
    if let Some(refused) = refused_twice(session) {
        return Some(Spin { signal: Signal::LandRefusal, evidence: refused });
    }
    judged(evidence)
}

/// The signals the ledger's records show for a run that ended not done with no harness cause recorded: a missed check
/// that reopened its box, twice its stated ETA, else a worker that got nowhere on its rung.
pub fn judged(evidence: &Evidence) -> Option<Spin> {
    evidence.end.filter(|end| end.open && !end.caused)?;
    if evidence.reopened {
        return Some(Spin { signal: Signal::Reopened, evidence: "a missed check reopened its box".into() });
    }
    if let Some(time) = evidence.time.filter(|time| time.eta_min > 0 && time.minutes >= 2 * time.eta_min) {
        return Some(Spin { signal: Signal::EtaOverrun, evidence: format!("took {} min against its {} min ETA", time.minutes, time.eta_min) });
    }
    Some(Spin { signal: Signal::Reasoned, evidence: "ended not done with no harness cause recorded".into() })
}

/// The lane the lander refused twice running with the same verdict since it last landed, the second time with
/// as many errors or more when the lane's own checks refused it.
fn refused_twice(session: &Value) -> Option<String> {
    let delivered = session["entries"].as_array()?.iter().filter(|entry| entry["kind"] == "user" || entry["kind"] == "notice").filter_map(|entry| Notice::delivered(entry["text"].as_str()?));
    let mut tries: BTreeMap<String, Vec<(String, Option<usize>)>> = BTreeMap::new();
    for notice in delivered {
        match notice {
            Notice::Landed { .. } => tries.clear(),
            Notice::Refused { branch, verdict, errors, .. } => tries.entry(branch).or_default().push((verdict, errors)),
        }
    }
    tries.into_iter().find_map(|(lane, refusals)| {
        let [.., (verdict, counted), (then_verdict, then)] = &refusals[..] else { return None };
        if verdict != then_verdict {
            return None;
        }
        match (counted, then) {
            (Some(before), Some(after)) if after >= before => Some(format!("lane {lane}, refused twice by its own landing checks without fewer errors ({before}, then {after})")),
            (Some(_), Some(_)) => None,
            (None, None) => Some(format!("lane {lane}, refused twice for the same reason: {then_verdict}")),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(report: &str) -> Value {
        serde_json::json!({"entries": [{"kind": "assistant", "text": report}], "turns": [{"exit": "completed", "ended_ms": 1_000_000}]})
    }

    fn with_notices(report: &str, notices: &[String]) -> Value {
        let mut entries: Vec<Value> = notices.iter().map(|notice| serde_json::json!({"kind": "user", "text": format!("[from service:north-land@whiterabbit] {notice}\nlast lines of the log:")})).collect();
        entries.push(serde_json::json!({"kind": "assistant", "text": report}));
        serde_json::json!({"entries": entries, "turns": [{"exit": "completed", "ended_ms": 1_000_000}]})
    }

    fn refused(verdict: &str, errors: Option<usize>) -> String {
        Notice::Refused { branch: "n940-verify".into(), head: "abc".into(), verdict: verdict.into(), errors }.text()
    }

    const OPEN: Option<RunEnd> = Some(RunEnd { open: true, caused: false });

    fn time(minutes: u64, eta_min: u64) -> Evidence {
        Evidence { reopened: false, time: Some(RunTime { minutes, eta_min }), end: OPEN }
    }

    #[test]
    fn the_same_land_refusal_twice_needs_the_same_verdict_and_no_fewer_errors_when_the_lane_s_checks_refused_it() {
        let failed = "Not done: the lane still fails its checks.\nStop: landing";
        assert_eq!(observed(&with_notices(failed, &[refused("failed land check cargo nextest run exited 100", Some(3)), refused("failed land check cargo nextest run exited 100", Some(3))]), &Evidence::default()).map(|spin| spin.signal), Some(Signal::LandRefusal));
        assert_eq!(observed(&with_notices(failed, &[refused("failed land check cargo nextest run exited 100", Some(3)), refused("failed land check cargo nextest run exited 100", Some(2))]), &Evidence::default()), None, "fewer errors keep the rung");
        assert_eq!(observed(&with_notices(failed, &[refused("failed land check cargo nextest run exited 100", Some(3))]), &Evidence::default()), None, "one refusal is no evidence");
        assert_eq!(observed(&with_notices(failed, &[refused("failed land check cargo nextest run exited 100", Some(3)), refused("failed land check cargo clippy exited 101", Some(3))]), &Evidence::default()), None, "a different refusal is no repeat of the same one");
        let landed = format!("[from service:north-land@whiterabbit] {}", Notice::Landed { branch: "n940-verify".into(), sha: "def".into() }.text());
        let mut entries: Vec<Value> = with_notices(failed, &[refused("failed land check cargo nextest run exited 100", Some(3)), refused("failed land check cargo nextest run exited 100", Some(3))])["entries"].as_array().cloned().unwrap_or_default();
        entries.push(serde_json::json!({"kind": "user", "text": landed}));
        assert_eq!(observed(&serde_json::json!({"entries": entries}), &Evidence::default()), None, "a landing after clears it");
    }

    #[test]
    fn a_lane_the_lander_refused_twice_for_the_same_reason_with_no_error_count_climbs_as_the_south_lock_did() {
        let held = refused("the south checkout is locked by another safe-push", None);
        let spin = observed(&with_notices("Not done: the lane is still queued.\nStop: landing", &[held.clone(), held]), &Evidence::default());
        assert_eq!(spin.as_ref().map(|spin| (spin.signal, spin.evidence.contains("the south checkout is locked by another safe-push"))), Some((Signal::LandRefusal, true)));
    }

    #[test]
    fn a_run_that_took_twice_the_eta_its_brief_stated_climbs_as_the_eighteen_minute_candidate_check_did() {
        let report = session("Not done: the lander's check took longer than the box.");
        assert_eq!(observed(&report, &time(36, 18)).map(|spin| spin.signal), Some(Signal::EtaOverrun));
        assert_eq!(observed(&report, &time(35, 18)).map(|spin| spin.signal), Some(Signal::Reasoned), "under twice the ETA it is the worker's not-done end");
    }

    #[test]
    fn the_climb_reads_the_settled_end_and_the_recorded_cause_and_never_the_report_north_872() {
        let reasoning = session("Not done: both rewrites left it failing.\nStop: reasoning");
        let at = |end: Option<RunEnd>| Evidence { end, ..time(20, 18) };
        assert_eq!(observed(&reasoning, &at(OPEN)).map(|spin| spin.signal), Some(Signal::Reasoned));
        assert_eq!(observed(&reasoning, &at(Some(RunEnd { open: false, caused: false }))), None, "a run whose lane landed is done whatever its report says");
        assert_eq!(observed(&reasoning, &at(Some(RunEnd { open: true, caused: true }))), None, "a recorded harness or Tom cause hands the failure on");
        assert_eq!(observed(&reasoning, &at(None)), None, "a run the ledger has not settled yet");
        assert_eq!(observed(&session("Not done: it waits on Tom.\nStop: decision"), &at(OPEN)).map(|spin| spin.signal), Some(Signal::Reasoned), "a Stop: decision line with no gate is the worker's");
        let reopened = Evidence { reopened: true, ..at(OPEN) };
        assert_eq!(observed(&reasoning, &reopened).map(|spin| spin.signal), Some(Signal::Reopened));
        assert_eq!(observed(&serde_json::json!({"entries": []}), &reopened), None, "a session with no report shows nothing");
        assert_eq!(Signal::Reasoned.key(), "reasoning");
        assert_eq!([Signal::LandRefusal.key(), Signal::Reopened.key(), Signal::EtaOverrun.key()], ["land-refusal", "reopened", "eta-overrun"]);
    }
}
