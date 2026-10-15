use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;

pub const QUESTION_ACK_SECONDS: i64 = 2 * 60;
pub const STATUS_STALE_SECONDS: i64 = 20 * 60;
pub const ARTIFACT_ACK_SECONDS: i64 = 10 * 60;
pub const LEAD_ACK_SECONDS: i64 = 10 * 60;
pub const WATCH_NODE: &str = "watch";
pub const PROXY_NODE: &str = "proxy";
pub const CANARY_NODE: &str = "watch-canary";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    Question,
    Stuck,
    Answer,
    Ack,
    Eta,
    Artifact,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    Tom,
    Proxy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub kind: EventKind,
    pub from: String,
    pub to: String,
    pub ts: Timestamp,
    #[serde(default)]
    pub answered_at: Option<Timestamp>,
    #[serde(default)]
    pub answer_of: Option<String>,
    #[serde(default)]
    pub acked_at: Option<Timestamp>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub audience: Option<Audience>,
    #[serde(default)]
    pub eta: Option<Timestamp>,
    #[serde(default)]
    pub done_check_result: Option<bool>,
    #[serde(default)]
    pub what: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Active,
    Idle,
    Done,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeRole {
    Lead,
    SubLead,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub state: NodeState,
    #[serde(default)]
    pub role: Option<NodeRole>,
    #[serde(default)]
    pub status_mtime: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stopgap {
    pub id: String,
    pub what: String,
    pub owner: String,
    pub deadline: Timestamp,
    pub match_tag: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UnknownDelivery {
    pub lead: String,
    pub since: Timestamp,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub stopgaps: Vec<Stopgap>,
    #[serde(default)]
    pub unknown_delivery: Vec<UnknownDelivery>,
    #[serde(default)]
    pub restart_storm: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Invariant {
    I1,
    I2,
    I3,
    I4,
    I5,
    I6,
}

impl Invariant {
    pub fn name(self) -> &'static str {
        match self {
            Self::I1 => "I1",
            Self::I2 => "I2",
            Self::I3 => "I3",
            Self::I4 => "I4",
            Self::I5 => "I5",
            Self::I6 => "I6",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub invariant: Invariant,
    pub subject: String,
    pub tom_level: bool,
    pub to: String,
    pub window: Timestamp,
    pub key: String,
}

impl Alarm {
    fn new(invariant: Invariant, subject: &str, tom_level: bool, to: &str, window: Timestamp) -> Self {
        let key = format!("watch:{}:{subject}:{}", invariant.name(), window.compact());
        Self { invariant, subject: subject.into(), tom_level, to: to.into(), window, key }
    }
}

fn by(time: Option<Timestamp>, now: Timestamp) -> bool {
    time.is_some_and(|time| time <= now)
}

fn question_window(snapshot: &Snapshot, event: &Event) -> Option<Timestamp> {
    let canary = event.to == CANARY_NODE;
    if event.from == WATCH_NODE && !canary {
        return None;
    }
    let quick = canary || event.to == PROXY_NODE || (event.kind == EventKind::Question && !event.options.is_empty());
    let lead = snapshot.nodes.iter().any(|node| {
        node.id == event.to && node.state == NodeState::Active && matches!(node.role, Some(NodeRole::Lead | NodeRole::SubLead))
    });
    if quick {
        Some(event.ts.plus_seconds(QUESTION_ACK_SECONDS))
    } else if lead {
        Some(event.ts.plus_seconds(LEAD_ACK_SECONDS))
    } else {
        None
    }
}

fn answered(snapshot: &Snapshot, event: &Event, now: Timestamp) -> bool {
    snapshot.events.iter().any(|answer| {
        answer.kind == EventKind::Answer && answer.ts <= now && answer.answer_of.as_deref() == Some(event.id.as_str())
    })
}

pub fn evaluate(snapshot: &Snapshot, now: Timestamp) -> Vec<Alarm> {
    let mut alarms = Vec::new();
    for event in snapshot.events.iter().filter(|event| event.ts <= now) {
        let closed = by(event.answered_at, now);
        match event.kind {
            EventKind::Question | EventKind::Stuck => {
                let canary = event.to == CANARY_NODE;
                if let Some(window) = question_window(snapshot, event)
                    && !closed && !answered(snapshot, event, now) && !by(event.acked_at, now) && now >= window {
                    let tom = event.audience == Some(Audience::Tom);
                    alarms.push(Alarm::new(Invariant::I1, &event.id, tom, if canary { CANARY_NODE } else { PROXY_NODE }, window));
                }
            }
            EventKind::Eta => {
                if let Some(eta) = event.eta
                    && now >= eta && !closed && event.done_check_result != Some(true) {
                    alarms.push(Alarm::new(Invariant::I3, &event.id, false, PROXY_NODE, eta));
                }
            }
            EventKind::Artifact => {
                let window = event.ts.plus_seconds(ARTIFACT_ACK_SECONDS);
                if !by(event.acked_at, now) && now >= window {
                    alarms.push(Alarm::new(Invariant::I4, &event.id, true, PROXY_NODE, window));
                }
            }
            EventKind::Answer | EventKind::Ack | EventKind::Other => {}
        }
    }
    for node in snapshot.nodes.iter().filter(|node| node.state == NodeState::Active) {
        if let Some(mtime) = node.status_mtime {
            let window = mtime.plus_seconds(STATUS_STALE_SECONDS);
            if now >= window {
                alarms.push(Alarm::new(Invariant::I2, &node.id, false, PROXY_NODE, window));
            }
        }
    }
    for stopgap in snapshot.stopgaps.iter().filter(|stopgap| now >= stopgap.deadline) {
        alarms.push(Alarm::new(Invariant::I5, &stopgap.id, true, PROXY_NODE, stopgap.deadline));
    }
    for delivery in snapshot.unknown_delivery.iter().filter(|delivery| now >= delivery.since) {
        alarms.push(Alarm::new(Invariant::I6, &delivery.lead, false, PROXY_NODE, delivery.since));
    }
    if let Some(since) = snapshot.restart_storm.filter(|since| now >= *since) {
        alarms.push(Alarm::new(Invariant::I6, "daemon", false, PROXY_NODE, since));
    }
    alarms.sort_by(|left, right| (left.invariant, &left.subject).cmp(&(right.invariant, &right.subject)));
    alarms
}

#[cfg(test)]
pub(crate) mod invariants {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    pub(crate) struct Fixture {
        pub start: Timestamp,
        pub minutes: i64,
        pub snapshot: Snapshot,
        pub expected: Vec<Expected>,
    }

    #[derive(Deserialize)]
    pub(crate) struct Expected {
        pub invariant: Invariant,
        pub subject: String,
        pub minute: i64,
        pub tom_level: bool,
        #[serde(default = "proxy")]
        pub to: String,
    }

    fn proxy() -> String {
        PROXY_NODE.into()
    }

    pub(crate) fn fixture() -> Fixture {
        serde_json::from_str(include_str!("fixtures/watch/minute_stepped.json")).unwrap()
    }

    pub(crate) fn first_fire(fixture: &Fixture) -> BTreeMap<(Invariant, String), (i64, Alarm)> {
        let mut first = BTreeMap::<(Invariant, String), (i64, Alarm)>::new();
        for minute in 0..=fixture.minutes {
            let now = fixture.start.plus_seconds(minute * 60);
            let alarms = evaluate(&fixture.snapshot, now);
            assert_eq!(alarms, evaluate(&fixture.snapshot, now), "minute {minute}: evaluation is not pure");
            for alarm in alarms {
                let entry = first.entry((alarm.invariant, alarm.subject.clone())).or_insert_with(|| (minute, alarm.clone()));
                assert_eq!(entry.1.key, alarm.key, "minute {minute}: key for {} {} moved", alarm.invariant.name(), alarm.subject);
            }
        }
        first
    }

    #[test]
    fn each_invariant_fires_at_its_threshold_minute() {
        nothing_fires_one_minute_before_threshold();
        dedup_key_is_stable_across_passes();
        tom_level_marks_audience_artifacts_and_stopgaps();
        let fixture = fixture();
        let first = first_fire(&fixture);
        let actual = first.iter().map(|((invariant, subject), (minute, alarm))| {
            (*invariant, subject.clone(), *minute, alarm.tom_level, alarm.to.clone())
        }).collect::<Vec<_>>();
        let mut expected = fixture.expected.iter().map(|expected| {
            (expected.invariant, expected.subject.clone(), expected.minute, expected.tom_level, expected.to.clone())
        }).collect::<Vec<_>>();
        expected.sort();
        assert_eq!(actual, expected);
        for invariant in [Invariant::I1, Invariant::I2, Invariant::I3, Invariant::I4, Invariant::I5, Invariant::I6] {
            assert!(actual.iter().any(|row| row.0 == invariant), "{} never fired", invariant.name());
        }
    }

    fn nothing_fires_one_minute_before_threshold() {
        let fixture = fixture();
        for expected in &fixture.expected {
            let before = fixture.start.plus_seconds((expected.minute - 1) * 60);
            let at = fixture.start.plus_seconds(expected.minute * 60);
            let fired = |now| evaluate(&fixture.snapshot, now).iter()
                .any(|alarm| alarm.invariant == expected.invariant && alarm.subject == expected.subject);
            assert!(!fired(before), "{} {} fired early", expected.invariant.name(), expected.subject);
            assert!(fired(at), "{} {} did not fire", expected.invariant.name(), expected.subject);
        }
    }

    fn dedup_key_is_stable_across_passes() {
        let fixture = fixture();
        let first = first_fire(&fixture);
        let end = fixture.start.plus_seconds(fixture.minutes * 60);
        for alarm in evaluate(&fixture.snapshot, end) {
            let (minute, earlier) = &first[&(alarm.invariant, alarm.subject.clone())];
            assert_eq!(alarm.key, earlier.key);
            assert_eq!(alarm.key, format!("watch:{}:{}:{}", alarm.invariant.name(), alarm.subject, alarm.window.compact()));
            assert_eq!(alarm.window, fixture.start.plus_seconds(minute * 60));
        }
    }

    fn tom_level_marks_audience_artifacts_and_stopgaps() {
        let first = first_fire(&fixture());
        for ((invariant, subject), (_, alarm)) in &first {
            let expected = matches!(invariant, Invariant::I4 | Invariant::I5) || subject == "q-tom";
            assert_eq!(alarm.tom_level, expected, "{} {subject}", invariant.name());
        }
    }

    fn roles_fixture() -> Fixture {
        serde_json::from_str(include_str!("fixtures/watch/i1_roles.json")).unwrap()
    }

    fn never_fires(subject: &str) {
        let fixture = roles_fixture();
        for minute in 0..=fixture.minutes {
            let now = fixture.start.plus_seconds(minute * 60);
            assert!(!evaluate(&fixture.snapshot, now).iter().any(|alarm| alarm.subject == subject), "{subject} fired at minute {minute}");
        }
    }

    #[test]
    fn stuck_to_lead_fires_at_ten_minutes_not_nine() {
        let fixture = roles_fixture();
        for expected in &fixture.expected {
            let alarm = |minute: i64| evaluate(&fixture.snapshot, fixture.start.plus_seconds(minute * 60)).into_iter()
                .find(|alarm| alarm.invariant == expected.invariant && alarm.subject == expected.subject);
            assert!(alarm(expected.minute - 1).is_none(), "{} fired early", expected.subject);
            let alarm = alarm(expected.minute).unwrap_or_else(|| panic!("{} did not fire", expected.subject));
            let window = fixture.start.plus_seconds(expected.minute * 60);
            assert_eq!((alarm.tom_level, alarm.to.as_str(), alarm.window), (expected.tom_level, expected.to.as_str(), window));
            assert_eq!(alarm.key, format!("watch:I1:{}:{}", expected.subject, window.compact()));
        }
    }

    #[test]
    fn stuck_from_watch_never_fires() {
        never_fires("stuck-watch");
    }

    #[test]
    fn stuck_to_inactive_node_never_fires() {
        never_fires("stuck-inactive");
    }

    #[test]
    fn answer_of_closes_row() {
        never_fires("stuck-answered");
    }
}
