use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{NorthError, NorthResult};

pub const QUESTION_ACK_SECONDS: i64 = 2 * 60;
pub const STATUS_STALE_SECONDS: i64 = 20 * 60;
pub const ARTIFACT_ACK_SECONDS: i64 = 10 * 60;
pub const LEAD_ACK_SECONDS: i64 = 10 * 60;
pub const WATCH_NODE: &str = "watch";
pub const PROXY_NODE: &str = "proxy";
pub const CANARY_NODE: &str = "watch-canary";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub fn parse(text: &str) -> NorthResult<Self> {
        let invalid = || NorthError::Usage(format!("Expected an ISO time like 2026-10-15T15:20:00Z, got {text:?}"));
        let bytes = text.as_bytes();
        if bytes.len() < 17 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't' | b' ') || bytes[13] != b':' {
            return Err(invalid());
        }
        let number = |range: std::ops::Range<usize>| -> NorthResult<i64> {
            let part = text.get(range).ok_or_else(invalid)?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) { return Err(invalid()); }
            part.parse().map_err(|_| invalid())
        };
        let (year, month, day, hour, minute) = (number(0..4)?, number(5..7)?, number(8..10)?, number(11..13)?, number(14..16)?);
        let mut rest = &text[16..];
        let mut second = 0;
        if let Some(after) = rest.strip_prefix(':') {
            second = number(17..19)?;
            rest = after.get(2..).ok_or_else(invalid)?;
            if let Some(fraction) = rest.strip_prefix('.') {
                let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
                if digits == 0 { return Err(invalid()); }
                rest = &fraction[digits..];
            }
        }
        let offset = match rest {
            "Z" | "z" => 0,
            _ if rest.len() == 6 && matches!(rest.as_bytes()[0], b'+' | b'-') && rest.as_bytes()[3] == b':' => {
                let start = text.len() - 5;
                let value = number(start..start + 2)? * 3600 + number(start + 3..start + 5)? * 60;
                if rest.starts_with('-') { -value } else { value }
            }
            _ => return Err(invalid()),
        };
        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) || hour > 23 || minute > 59 || second > 60 {
            return Err(invalid());
        }
        Ok(Self(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset))
    }

    pub fn iso(self) -> String {
        let (year, month, day, hour, minute, second) = self.civil();
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    pub fn compact(self) -> String {
        let (year, month, day, hour, minute, _) = self.civil();
        format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}Z")
    }

    fn civil(self) -> (i64, i64, i64, i64, i64, i64) {
        let days = self.0.div_euclid(86_400);
        let seconds = self.0.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        (year, month, day, seconds / 3600, seconds % 3600 / 60, seconds % 60)
    }

    pub fn now() -> Self {
        let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Self(i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.iso())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

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
        Some(Timestamp(event.ts.0 + QUESTION_ACK_SECONDS))
    } else if lead {
        Some(Timestamp(event.ts.0 + LEAD_ACK_SECONDS))
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
                let window = Timestamp(event.ts.0 + ARTIFACT_ACK_SECONDS);
                if !by(event.acked_at, now) && now >= window {
                    alarms.push(Alarm::new(Invariant::I4, &event.id, true, PROXY_NODE, window));
                }
            }
            EventKind::Answer | EventKind::Ack | EventKind::Other => {}
        }
    }
    for node in snapshot.nodes.iter().filter(|node| node.state == NodeState::Active) {
        if let Some(mtime) = node.status_mtime {
            let window = Timestamp(mtime.0 + STATUS_STALE_SECONDS);
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
            let now = Timestamp(fixture.start.0 + minute * 60);
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
            let before = Timestamp(fixture.start.0 + (expected.minute - 1) * 60);
            let at = Timestamp(fixture.start.0 + expected.minute * 60);
            let fired = |now| evaluate(&fixture.snapshot, now).iter()
                .any(|alarm| alarm.invariant == expected.invariant && alarm.subject == expected.subject);
            assert!(!fired(before), "{} {} fired early", expected.invariant.name(), expected.subject);
            assert!(fired(at), "{} {} did not fire", expected.invariant.name(), expected.subject);
        }
    }

    fn dedup_key_is_stable_across_passes() {
        let fixture = fixture();
        let first = first_fire(&fixture);
        let end = Timestamp(fixture.start.0 + fixture.minutes * 60);
        for alarm in evaluate(&fixture.snapshot, end) {
            let (minute, earlier) = &first[&(alarm.invariant, alarm.subject.clone())];
            assert_eq!(alarm.key, earlier.key);
            assert_eq!(alarm.key, format!("watch:{}:{}:{}", alarm.invariant.name(), alarm.subject, alarm.window.compact()));
            assert_eq!(alarm.window, Timestamp(fixture.start.0 + minute * 60));
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
            let now = Timestamp(fixture.start.0 + minute * 60);
            assert!(!evaluate(&fixture.snapshot, now).iter().any(|alarm| alarm.subject == subject), "{subject} fired at minute {minute}");
        }
    }

    #[test]
    fn stuck_to_lead_fires_at_ten_minutes_not_nine() {
        let fixture = roles_fixture();
        for expected in &fixture.expected {
            let alarm = |minute: i64| evaluate(&fixture.snapshot, Timestamp(fixture.start.0 + minute * 60)).into_iter()
                .find(|alarm| alarm.invariant == expected.invariant && alarm.subject == expected.subject);
            assert!(alarm(expected.minute - 1).is_none(), "{} fired early", expected.subject);
            let alarm = alarm(expected.minute).unwrap_or_else(|| panic!("{} did not fire", expected.subject));
            let window = Timestamp(fixture.start.0 + expected.minute * 60);
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

    #[test]
    fn timestamps_round_trip_iso() {
        let time = Timestamp::parse("2026-10-15T15:20:00Z").unwrap();
        assert_eq!(time.iso(), "2026-10-15T15:20:00Z");
        assert_eq!(time.compact(), "20261015T1520Z");
        assert_eq!(Timestamp::parse("2026-10-15T17:20:00.5+02:00").unwrap(), time);
        assert_eq!(Timestamp::parse("2024-02-29T00:00:00Z").unwrap().iso(), "2024-02-29T00:00:00Z");
        assert_eq!(Timestamp::parse("1970-01-01T00:00:00Z").unwrap(), Timestamp(0));
        for bad in ["2026-10-15", "2026-02-30T00:00:00Z", "2026-10-15T15:20:00", "ACK 7751 15:20", "2026-10-15T15:20:00+0200"] {
            assert!(Timestamp::parse(bad).is_err(), "{bad}");
        }
    }
}
