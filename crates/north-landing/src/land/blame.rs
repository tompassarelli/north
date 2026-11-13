use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::after::follow;
use super::checks::Verdict;

const ASK_LIMIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Suspects {
    Untraced,
    NoSuspect,
    Named(Vec<usize>),
}

/// One prefix check: its verdict and the keys of the failures it reached, so a missing failure tells a later lane's culprit from an earlier one's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Probe {
    pub verdict: Verdict,
    pub failing: BTreeSet<String>,
}

impl Probe {
    fn introduced(&self, signature: &str) -> Option<bool> {
        match &self.verdict {
            Verdict::RedMain(_) => Some(self.failing.contains(signature)),
            Verdict::Green => Some(false),
            _ => None,
        }
    }
}

const MAP_HORIZON_SECS: u64 = 3 * 24 * 60 * 60;

#[derive(Default, serde::Deserialize)]
struct Answer {
    #[serde(default)]
    lanes: Vec<String>,
    #[serde(default)]
    known: BTreeMap<String, bool>,
    #[serde(default)]
    edges: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    untraced: Vec<String>,
    map_age_secs: Option<u64>,
}

/// Reads `north graph ask blame --json` and says why. Exit 0 names lanes, kept even when they are every lane; exit 3 is
/// `NoSuspect` only when every failure is a test the map knows, the map is at most 3 days old and no lane is untraced;
/// anything else (exit 2, a name that is no landed lane, unreadable output) is `Untraced`.
pub fn read(lanes: &[String], exit: Option<i32>, printed: &str) -> (Suspects, String) {
    let Ok(answer) = serde_json::from_str::<Answer>(printed) else {
        return (Suspects::Untraced, format!("the graph answered exit {exit:?} with no readable blame"));
    };
    match exit {
        Some(0) => named(lanes, &answer),
        Some(3) => no_suspect(&answer),
        _ => (Suspects::Untraced, format!("the graph answered exit {exit:?}")),
    }
}

fn named(lanes: &[String], answer: &Answer) -> (Suspects, String) {
    let mut at: Vec<usize> = Vec::new();
    for name in &answer.lanes {
        let Some(index) = lanes.iter().position(|lane| lane == name) else {
            return (Suspects::Untraced, format!("the graph named {name}, which is no landed lane"));
        };
        at.push(index);
    }
    at.sort_unstable();
    at.dedup();
    if at.is_empty() {
        return (Suspects::Untraced, "the graph named no lane".to_owned());
    }
    let through: Vec<String> = answer.edges.iter().flat_map(|(failure, edges)| edges.iter().map(move |(lane, edge)| format!("{lane} through the {edge} edge for {failure}"))).collect();
    let why = if through.is_empty() { at.iter().map(|&index| lanes[index].as_str()).collect::<Vec<_>>().join(", ") } else { through.join(", ") };
    (Suspects::Named(at), why)
}

fn no_suspect(answer: &Answer) -> (Suspects, String) {
    let unknown: Vec<&str> = answer.known.iter().filter(|(_, known)| !**known).map(|(failure, _)| failure.as_str()).collect();
    if answer.known.is_empty() || !unknown.is_empty() {
        return (Suspects::Untraced, format!("no lane holds the failures, but the graph does not know {}", if unknown.is_empty() { "any".to_owned() } else { unknown.join(", ") }));
    }
    match answer.map_age_secs {
        Some(age) if age <= MAP_HORIZON_SECS && answer.untraced.is_empty() => (Suspects::NoSuspect, format!("no lane reaches the known {}", answer.known.keys().cloned().collect::<Vec<_>>().join(", "))),
        Some(age) if age <= MAP_HORIZON_SECS => (Suspects::Untraced, format!("{} is untraced", answer.untraced.join(", "))),
        _ => (Suspects::Untraced, "the coverage map is older than 3 days or unrecorded".to_owned()),
    }
}

pub fn suspects(lanes: &[String], exit: Option<i32>, printed: &str) -> Suspects {
    read(lanes, exit, printed).0
}

pub fn ranges(base: &str, lanes: &[String], tips: &[String]) -> Vec<String> {
    lanes.iter().zip(tips).enumerate().map(|(at, (lane, tip))| format!("{lane}={}..{tip}", if at == 0 { base } else { &tips[at - 1] })).collect()
}

pub fn ask(repo: &Path, log: &Path, base: &str, head: &str, lanes: &[String], tips: &[String], room: usize) -> Suspects {
    let mut command = Command::new(follow::program());
    command.args(["graph", "ask", "blame", "--json", "--log"]).arg(log).current_dir(repo).stdin(Stdio::null());
    command.args(ranges(base, lanes, tips));
    let asked = north_base::platform::process::run_command_within(command, None, ASK_LIMIT);
    let (suspects, why) = asked.as_ref().map_or((Suspects::Untraced, "the graph could not be asked".to_owned()), |output| read(lanes, output.status.code(), &String::from_utf8_lossy(&output.stdout)));
    match &suspects {
        Suspects::Named(named) if named.len() == 1 => println!("north land: the change graph names {} (one suspect: {why}) for the red after-landing run of {head}, so it is blamed once its parent passes the failed checks", lanes[named[0]]),
        Suspects::Named(named) => println!("north land: the change graph names {} of the {} lanes of {head} ({why}), so the culprits are bisected among {}", named.len(), lanes.len(), named.iter().map(|&at| lanes[at].as_str()).collect::<Vec<_>>().join(", ")),
        Suspects::NoSuspect => println!("north land: the change graph trusts that no lane of {head} caused the red after-landing run: {why}"),
        Suspects::Untraced if room < lanes.len() => println!("north land: the change graph is untraced for the red after-landing run of {head} ({why}), and {room} offload slots are free for its {} prefixes, so the whole batch is bisected", lanes.len()),
        Suspects::Untraced => println!("north land: the change graph is untraced for the red after-landing run of {head} ({why}), so the whole batch is bisected"),
    }
    suspects
}

/// The outcome of a prefix search: the first lane red where its parent passes, a base already red on the failed
/// checks, or no verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Blame {
    Lane(usize),
    BaseRed,
    Unknown,
}

struct Prefixes<R> {
    seen: BTreeMap<usize, Verdict>,
    round: R,
}

impl<R: FnMut(&[usize]) -> Vec<Verdict>> Prefixes<R> {
    fn probe(&mut self, counts: &[usize]) -> Vec<Verdict> {
        let fresh: Vec<usize> = counts.iter().copied().filter(|count| !self.seen.contains_key(count)).collect::<BTreeSet<_>>().into_iter().collect();
        if !fresh.is_empty() {
            let verdicts = (self.round)(&fresh);
            self.seen.extend(fresh.into_iter().zip(verdicts));
        }
        counts.iter().map(|count| self.seen.get(count).cloned().unwrap_or_else(|| Verdict::Transient(format!("no verdict for the first {count} lanes")))).collect()
    }

    fn red(&mut self, count: usize) -> Option<bool> {
        red(self.probe(&[count]).pop())
    }
}

/// Searches the prefixes of a red batch of `lanes` landings for the first red one; `round` checks each prefix, by its
/// count of landed lanes (0 is the base), on the failed checks alone. The red run is the full prefix's check, so it is
/// seeded and costs no probe. A lane is blamed only once its parent prefix is seen to pass, so a single graph-named
/// suspect is probed on its parent and a base red on the failed checks is `BaseRed`, never a lane of this batch.
pub fn culprit(lanes: usize, room: usize, suspects: &Suspects, round: impl FnMut(&[usize]) -> Vec<Verdict>) -> Blame {
    if lanes == 0 {
        return Blame::Unknown;
    }
    let mut prefixes = Prefixes { seen: BTreeMap::from([(lanes, Verdict::RedMain("the red after-landing run of the head".to_owned()))]), round };
    let found = match suspects {
        Suspects::Named(named) if named.len() == 1 => Some(named[0]),
        Suspects::Named(named) if !named.is_empty() => among(named, room, &mut prefixes),
        _ => batch(0, lanes, room, &mut prefixes),
    };
    confirmed(found, lanes, room, &mut prefixes)
}

/// Blames `found` only once its parent prefix passes and its own prefix is red, searching again below or above it
/// otherwise; prefix verdicts are remembered, so each search adds facts and the bound only stops a flapping check.
fn confirmed<R: FnMut(&[usize]) -> Vec<Verdict>>(mut found: Option<usize>, lanes: usize, room: usize, prefixes: &mut Prefixes<R>) -> Blame {
    for _ in 0..=lanes {
        let Some(lane) = found else { return Blame::Unknown };
        let verdicts = prefixes.probe(&[lane, lane + 1]);
        let Some(own) = verdicts.get(1).cloned() else { return Blame::Unknown };
        match (red(verdicts.first().cloned()), red(Some(own))) {
            (Some(true), _) if lane == 0 => return Blame::BaseRed,
            (Some(true), _) => found = batch(0, lane, room, prefixes),
            (Some(false), Some(true)) => return Blame::Lane(lane),
            (Some(false), Some(false)) if lane + 1 == lanes => return Blame::Unknown,
            (Some(false), Some(false)) => found = batch(lane + 1, lanes, room, prefixes),
            _ => return Blame::Unknown,
        }
    }
    Blame::Unknown
}

/// The lane whose prefix is the first red one after the prefix count `green`, `red_count` being a red one.
fn batch<R: FnMut(&[usize]) -> Vec<Verdict>>(green: usize, red_count: usize, room: usize, prefixes: &mut Prefixes<R>) -> Option<usize> {
    if room >= red_count - green {
        let counts: Vec<usize> = (green..red_count).collect();
        for (count, verdict) in counts.iter().zip(prefixes.probe(&counts)) {
            if red(Some(verdict))? {
                return Some(count.saturating_sub(1));
            }
        }
        return Some(red_count - 1);
    }
    bisected(green, red_count, |count| prefixes.red(count)).map(|red| red - 1)
}

/// Walks back from a base red on the failed checks over the landings before it, `earlier` of them newest first, `room`
/// at a time, to the first landing red where its parent passes: 0 is the base itself, `n` the nth landing before it.
pub fn walk_back(earlier: usize, room: usize, mut round: impl FnMut(&[usize]) -> Vec<Verdict>) -> Option<usize> {
    let mut next = 1;
    while next <= earlier {
        let counts: Vec<usize> = (next..=earlier.min(next + room.max(1) - 1)).collect();
        for (at, verdict) in counts.iter().zip(round(&counts)) {
            if !red(Some(verdict))? {
                return Some(at - 1);
            }
        }
        next += counts.len();
    }
    None
}

/// Every suspect whose prefix first reaches one of the red run's failure signatures: the signatures are searched level by level, one binary search each, and every prefix a search needs is checked once. Two lanes that break different checks are therefore isolated in two runs per level, at most k·log2(n) runs for k such lanes, without a revert between the searches. None when a prefix reached no verdict or a signature first appears outside the suspects, so the caller falls back to the single-culprit search.
pub fn culprits(lanes: usize, suspects: &Suspects, signatures: &BTreeSet<String>, mut round: impl FnMut(&[usize]) -> Vec<Probe>) -> Option<Vec<usize>> {
    let named: Vec<usize> = match suspects {
        Suspects::Named(named) if !named.is_empty() => named.clone(),
        _ => (0..lanes).collect(),
    };
    if named.is_empty() {
        return Some(Vec::new());
    }
    if named.len() == 1 || signatures.is_empty() {
        return None;
    }
    let mut searches: Vec<(&str, usize, usize)> = signatures.iter().map(|signature| (signature.as_str(), 0, named.len())).collect();
    // The run that went red is already the full prefix's check; seeding it costs no run and keeps the search inside k·log2(n).
    let mut checked: BTreeMap<usize, Probe> = BTreeMap::from([(lanes, Probe { verdict: Verdict::RedMain(String::new()), failing: signatures.clone() })]);
    loop {
        let mut wanted: Vec<usize> = searches.iter().filter(|(_, green, red)| green < red).map(|(_, green, red)| named[green + (red - green) / 2] + 1).filter(|count| !checked.contains_key(count)).collect();
        wanted.sort_unstable();
        wanted.dedup();
        if !wanted.is_empty() {
            let probes = round(&wanted);
            if probes.len() != wanted.len() {
                return None;
            }
            checked.extend(wanted.into_iter().zip(probes));
        }
        let mut active = false;
        for (signature, green, red) in &mut searches {
            if green >= red {
                continue;
            }
            let middle = *green + (*red - *green) / 2;
            match checked.get(&(named[middle] + 1))?.introduced(signature)? {
                true => *red = middle,
                false => *green = middle + 1,
            }
            active = true;
        }
        if !active {
            break;
        }
    }
    let mut found = Vec::new();
    for (signature, green, _) in searches {
        if green >= named.len() {
            println!("north land: the failing check {signature} first appears outside the lanes the change graph blames, so no lane is named for it");
            return None;
        }
        let lane = named[green];
        if !found.contains(&lane) {
            found.push(lane);
        }
    }
    Some(found)
}

fn among<R: FnMut(&[usize]) -> Vec<Verdict>>(named: &[usize], room: usize, prefixes: &mut Prefixes<R>) -> Option<usize> {
    let through = |at: usize| named[at] + 1;
    if room + 1 >= named.len() {
        let verdicts = prefixes.probe(&(0..named.len() - 1).map(through).collect::<Vec<_>>());
        for (at, verdict) in verdicts.into_iter().enumerate() {
            if red(Some(verdict))? {
                return Some(named[at]);
            }
        }
        return named.last().copied();
    }
    bisected(0, named.len(), |at| prefixes.red(through(at - 1))).map(|red| named[red - 1])
}

fn red(verdict: Option<Verdict>) -> Option<bool> {
    match verdict {
        Some(Verdict::RedMain(_)) => Some(true),
        Some(Verdict::Green) => Some(false),
        _ => None,
    }
}

fn bisected(mut green: usize, mut red: usize, mut red_at: impl FnMut(usize) -> Option<bool>) -> Option<usize> {
    while red - green > 1 {
        let middle = green + (red - green) / 2;
        if red_at(middle)? {
            red = middle;
        } else {
            green = middle;
        }
    }
    Some(red)
}

#[cfg(test)]
#[path = "blame_tests.rs"]
mod tests;
