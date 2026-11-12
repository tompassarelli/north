//! `north box path REPO#N [--box K]`: the states one box passed through from its claim to the runtime that holds its
//! landing, each with when it began, the time spent in it and the record it is read from; a state no record on this
//! host holds prints as unrecorded with the length of the gap it lies in (north#1235, docs/rationale/lifecycle-deadlines.md).
mod records;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::{Timestamp, local_offset_seconds};

const USAGE: &str = "usage: north box path REPO#N [--box K]";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Phase {
    Claimed,
    Launching,
    Working,
    HandingIn,
    Queued,
    Checking,
    Landed,
    Reported,
    Delivered,
    Verified,
    Promoted,
}

impl Phase {
    const PATH: [Self; 11] = [Self::Claimed, Self::Launching, Self::Working, Self::HandingIn, Self::Queued, Self::Checking, Self::Landed, Self::Reported, Self::Delivered, Self::Verified, Self::Promoted];

    fn name(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Launching => "launching",
            Self::Working => "working",
            Self::HandingIn => "handing-in",
            Self::Queued => "queued",
            Self::Checking => "checking",
            Self::Landed => "landed",
            Self::Reported => "reported",
            Self::Delivered => "delivered",
            Self::Verified => "after-landing",
            Self::Promoted => "promoted",
        }
    }
}

/// When a state began, as one record says.
pub(super) struct Mark {
    pub phase: Phase,
    pub at_ms: i64,
    pub source: String,
}

/// One box's try as the records tell it: the states they date and, for each state they do not, why.
pub(super) struct BoxPath {
    pub title: String,
    pub marks: Vec<Mark>,
    pub missing: Vec<(Phase, String)>,
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let (item, wanted) = parse(arguments)?;
    let paths = records::paths(&item, wanted.as_deref())?;
    if paths.is_empty() {
        return Err(NorthError::Usage(format!("north box path: no worker run of {item}{} is in the threads ledger", wanted.map(|text| format!(" box {text:?}")).unwrap_or_default())));
    }
    let now = Timestamp::now().unix_millis();
    let blocks: Vec<String> = paths.iter().map(|path| render(path, now).join("\n")).collect();
    println!("{}", blocks.join("\n\n"));
    Ok(())
}

fn parse(arguments: &[String]) -> NorthResult<(String, Option<String>)> {
    let usage = || NorthError::Usage(USAGE.into());
    let (verb, rest) = arguments.split_first().ok_or_else(usage)?;
    let (item, flags) = rest.split_first().filter(|_| verb == "path").ok_or_else(usage)?;
    let numbered = item.split_once('#').is_some_and(|(repo, number)| !repo.is_empty() && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()));
    if !numbered {
        return Err(usage());
    }
    match flags {
        [] => Ok((item.clone(), None)),
        [flag, text] if flag == "--box" && !text.trim().is_empty() => Ok((item.clone(), Some(text.clone()))),
        _ => Err(usage()),
    }
}

fn render(path: &BoxPath, now_ms: i64) -> Vec<String> {
    let mut marks: Vec<&Mark> = path.marks.iter().collect();
    marks.sort_by_key(|mark| (mark.at_ms, mark.phase));
    let start = path.marks.iter().find(|mark| mark.phase == Phase::Claimed).map_or_else(|| marks.first().map_or(now_ms, |mark| mark.at_ms), |claim| claim.at_ms);
    if let Some(index) = marks.iter().position(|mark| mark.phase == Phase::Claimed) {
        let claim = marks.remove(index);
        marks.insert(0, claim);
    }
    let after = marks.iter().position(|mark| mark.phase == Phase::Promoted).map_or_else(Vec::new, |end| marks.split_off(end + 1));
    let shown: Vec<i64> = marks.iter().map(|mark| mark.at_ms.max(start)).collect();
    let mut lines = vec![path.title.clone()];
    for (phase, why) in path.missing.iter().filter(|(phase, _)| predecessor(path, *phase).is_none()) {
        lines.push(line(phase.name(), "unrecorded", "", &format!("before the first record: {why}")));
    }
    for (index, mark) in marks.iter().enumerate() {
        let end = shown.get(index + 1).copied();
        let last = end.is_none();
        let spent = match end {
            Some(end) => span(end - shown[index]),
            None if mark.phase == Phase::Promoted => "end".to_owned(),
            None => format!("{} so far", span(now_ms - shown[index])),
        };
        let early = if mark.at_ms < start { format!(", recorded {} before the claim", span(start - mark.at_ms)) } else { String::new() };
        lines.push(line(mark.phase.name(), &clock(shown[index]), &spent, &format!("{}{early}", mark.source)));
        let gap = end.unwrap_or(now_ms) - shown[index];
        for (phase, why) in path.missing.iter().filter(|(phase, _)| predecessor(path, *phase) == Some(mark.phase)) {
            let within = if last && mark.phase != Phase::Promoted { format!("in the {} since {} began", span(gap), mark.phase.name()) } else { format!("in the {} gap after {}", span(gap), mark.phase.name()) };
            lines.push(line(phase.name(), "unrecorded", "", &format!("{within}: {why}")));
        }
    }
    lines.push(total(&marks, &shown, start));
    for mark in after {
        lines.push(line(mark.phase.name(), &clock(mark.at_ms), "after promotion", &mark.source));
        for (phase, why) in path.missing.iter().filter(|(phase, _)| predecessor(path, *phase) == Some(mark.phase)) {
            lines.push(line(phase.name(), "unrecorded", "after promotion", why));
        }
    }
    lines
}

fn predecessor(path: &BoxPath, phase: Phase) -> Option<Phase> {
    Phase::PATH.iter().copied().take_while(|step| *step != phase).filter(|step| path.marks.iter().any(|mark| mark.phase == *step)).last()
}

fn total(marks: &[&Mark], shown: &[i64], start: i64) -> String {
    let summed: i64 = shown.windows(2).map(|pair| pair[1] - pair[0]).sum();
    let end = shown.last().copied().unwrap_or(start);
    let to = if marks.last().is_some_and(|mark| mark.phase == Phase::Promoted) { "promotion" } else { "the last record" };
    format!("claim to {to}: {}; the states above sum to {}", span(end - start), span(summed))
}

fn line(state: &str, at: &str, spent: &str, source: &str) -> String {
    format!("  {state:<14}{at:>12}  {spent:>16}  {source}")
}

fn clock(ms: i64) -> String {
    let seconds = (ms.div_euclid(1000) + local_offset_seconds()).rem_euclid(86_400);
    let (hour, minute, second) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    format!("{}:{minute:02}:{second:02} {}", (hour + 11) % 12 + 1, if hour < 12 { "AM" } else { "PM" })
}

fn span(ms: i64) -> String {
    let tenths = (ms.max(0) + 50) / 100;
    let (hours, minutes, seconds) = (tenths / 36_000, tenths / 600 % 60, (tenths % 600) as f64 / 10.0);
    match (hours, minutes) {
        (0, 0) => format!("{seconds:.1} s"),
        (0, _) => format!("{minutes} min {seconds:04.1} s"),
        _ => format!("{hours} h {minutes:02} min {seconds:04.1} s"),
    }
}
