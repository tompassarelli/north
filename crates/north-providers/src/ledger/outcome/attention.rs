use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

static REFERENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:[A-Za-z0-9_.-]+/)?([A-Za-z][A-Za-z0-9_-]*)#([0-9]+)\b").expect("reference pattern"));

pub const DETECTORS: [&str; 7] = ["watchdog:", "north land:", "health:", "agents flow:", "landing watch:", "red main", "red-main"];
const DAY_S: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Tom,
    Detector,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub session: String,
    pub at: i64,
    pub speaker: Speaker,
    pub names: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    pub id: i64,
    pub session: String,
    pub at: i64,
    pub text: String,
}

pub fn references(text: &str) -> BTreeSet<String> {
    REFERENCE.captures_iter(text).map(|found| format!("{}#{}", found[1].to_lowercase(), &found[2])).collect()
}

fn delivered_body(text: &str) -> &str {
    let mut rest = text.trim_start();
    while let Some(tail) = rest.strip_prefix('[').and_then(|tail| tail.split_once(']')).map(|(_, tail)| tail.trim_start()) {
        rest = tail;
    }
    rest
}

pub fn detector(text: &str) -> bool {
    let body = delivered_body(text);
    DETECTORS.iter().any(|detector| body.starts_with(detector))
}

pub fn messages(tom: &[Typed], delivered: &[(String, i64, String)]) -> Messages {
    let typed = tom.iter().map(|typed| Message { session: typed.session.clone(), at: typed.at, speaker: Speaker::Tom, names: references(&typed.text) });
    let alerts = delivered.iter().filter(|(_, _, text)| detector(text)).map(|(session, at, text)| Message { session: session.clone(), at: *at, speaker: Speaker::Detector, names: references(text) });
    let mut found: Vec<Message> = typed.chain(alerts).collect();
    found.sort_by(|left, right| (left.at, &left.session, left.speaker == Speaker::Tom).cmp(&(right.at, &right.session, right.speaker == Speaker::Tom)));
    Messages(found)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Correction {
    pub at: i64,
    pub names: BTreeSet<String>,
}

impl Correction {
    pub fn from_row(at: i64, kind: &str, source: &str, quote: &str, tom: &BTreeSet<i64>) -> Option<Self> {
        let mut words = source.split(' ');
        let speaker = words.next() == Some("tom");
        let message = words.next().and_then(|word| word.strip_prefix("convo:")).and_then(|id| id.parse::<i64>().ok());
        let typed = speaker && ["correction", "direction"].contains(&kind) && message.is_some_and(|id| tom.contains(&id));
        typed.then(|| Self { at, names: references(quote) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bug {
    pub item: String,
    pub at: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attention {
    pub messages: usize,
    pub conversations: usize,
    pub minutes: f64,
    pub active_days: usize,
    pub corrections: usize,
}

impl Attention {
    pub fn line(&self) -> String {
        let per_day = if self.active_days > 0 { format!("{:.1}", self.corrections as f64 / self.active_days as f64) } else { "n/a".into() };
        format!(
            "Tom's attention {} messages over {} conversations, {:.1} h from first to last message per conversation, {} active days; corrections per active day {per_day} ({}/{})",
            self.messages,
            self.conversations,
            self.minutes / 60.0,
            self.active_days,
            self.corrections,
            self.active_days,
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Incidents {
    pub corrections: usize,
    pub bugs: usize,
    pub tom: usize,
    pub detector: usize,
    pub neither: usize,
}

impl Incidents {
    pub fn line(&self) -> String {
        let total = self.corrections + self.bugs;
        format!(
            "incidents {total} ({} corrections, {} filed bugs): first noticed by Tom {}/{total}, by a detector {}/{total}, named by neither {}/{total}",
            self.corrections, self.bugs, self.tom, self.detector, self.neither
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Messages(pub Vec<Message>);

impl Messages {
    fn first(&self, speaker: Speaker, names: &BTreeSet<String>) -> Option<i64> {
        self.0.iter().filter(|message| message.speaker == speaker && !message.names.is_disjoint(names)).map(|message| message.at).next()
    }

    pub fn attention(&self, corrections: &[Correction], start: i64, end: i64) -> Attention {
        let mut spans: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
        let mut days = BTreeSet::new();
        let mut messages = 0;
        for message in self.0.iter().filter(|message| message.speaker == Speaker::Tom && start <= message.at && message.at < end) {
            messages += 1;
            days.insert(message.at.div_euclid(DAY_S));
            let span = spans.entry(message.session.as_str()).or_insert((message.at, message.at));
            *span = (span.0.min(message.at), span.1.max(message.at));
        }
        Attention {
            messages,
            conversations: spans.len(),
            minutes: spans.values().fold(0.0, |minutes, (first, last)| minutes + (last - first) as f64 / 60.0),
            active_days: days.len(),
            corrections: corrections.iter().filter(|correction| start <= correction.at && correction.at < end).count(),
        }
    }

    pub fn incidents(&self, corrections: &[Correction], bugs: &[Bug], start: i64, end: i64) -> Incidents {
        let mut incidents = Incidents::default();
        let mut judge = |tom: Option<i64>, detector: Option<i64>| match (tom, detector) {
            (_, Some(detector)) if tom.is_none_or(|tom| detector < tom) => incidents.detector += 1,
            (Some(_), _) => incidents.tom += 1,
            _ => incidents.neither += 1,
        };
        let within = |at: i64| start <= at && at < end;
        for correction in corrections.iter().filter(|correction| within(correction.at)) {
            let tom = self.first(Speaker::Tom, &correction.names).map_or(correction.at, |at| at.min(correction.at));
            judge(Some(tom), self.first(Speaker::Detector, &correction.names));
        }
        for bug in bugs.iter().filter(|bug| within(bug.at)) {
            let names = references(&bug.item);
            judge(self.first(Speaker::Tom, &names), self.first(Speaker::Detector, &names));
        }
        incidents.corrections = corrections.iter().filter(|correction| within(correction.at)).count();
        incidents.bugs = bugs.iter().filter(|bug| within(bug.at)).count();
        incidents
    }
}
