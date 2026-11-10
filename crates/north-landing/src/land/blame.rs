use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::after::follow;
use super::checks::Verdict;

const ASK_LIMIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Suspects {
    Batch,
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

/// Reads `north graph ask blame`: exit 0 prints the candidate lanes by name, one per line; exit 3 (untraced), exit 2
/// (no such verb), any other exit or a name that is no landed lane leaves the whole batch suspect.
pub fn suspects(lanes: &[String], exit: Option<i32>, printed: &str) -> Suspects {
    if exit != Some(0) {
        return Suspects::Batch;
    }
    let mut named = Vec::new();
    for name in printed.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let before = named.len();
        named.extend(lanes.iter().enumerate().filter(|(_, lane)| *lane == name).map(|(at, _)| at));
        if named.len() == before {
            return Suspects::Batch;
        }
    }
    named.sort_unstable();
    named.dedup();
    if named.is_empty() || named.len() == lanes.len() { Suspects::Batch } else { Suspects::Named(named) }
}

pub fn ranges(base: &str, lanes: &[String], tips: &[String]) -> Vec<String> {
    lanes.iter().zip(tips).enumerate().map(|(at, (lane, tip))| format!("{lane}={}..{tip}", if at == 0 { base } else { &tips[at - 1] })).collect()
}

pub fn ask(repo: &Path, log: &Path, base: &str, head: &str, lanes: &[String], tips: &[String], room: usize) -> Suspects {
    let mut command = Command::new(follow::program());
    command.args(["graph", "ask", "blame", "--log"]).arg(log).current_dir(repo).stdin(Stdio::null());
    command.args(ranges(base, lanes, tips));
    let asked = north_base::platform::process::run_command_within(command, None, ASK_LIMIT);
    let suspects = asked.as_ref().map_or(Suspects::Batch, |output| self::suspects(lanes, output.status.code(), &String::from_utf8_lossy(&output.stdout)));
    match &suspects {
        Suspects::Named(named) if named.len() == 1 => println!("north land: the change graph blames {} alone for the red after-landing run of {head}, so it is reverted with no bisect round", lanes[named[0]]),
        Suspects::Named(named) => println!("north land: the change graph blames {} of the {} lanes of {head}, so the culprits are bisected among {}", named.len(), lanes.len(), named.iter().map(|&at| lanes[at].as_str()).collect::<Vec<_>>().join(", ")),
        Suspects::Batch if room < lanes.len() => println!("north land: the change graph names no culprit for the red after-landing run of {head}, and {room} offload slots are free for its {} prefixes, so the whole batch is bisected", lanes.len()),
        Suspects::Batch => println!("north land: the change graph names no culprit for the red after-landing run of {head}, so the whole batch is bisected"),
    }
    suspects
}

pub fn culprit(lanes: usize, room: usize, suspects: &Suspects, mut round: impl FnMut(&[usize]) -> Vec<Verdict>) -> Option<usize> {
    if lanes == 0 {
        return None;
    }
    match suspects {
        Suspects::Named(named) if named.len() == 1 => return named.first().copied(),
        Suspects::Named(named) if !named.is_empty() => return among(named, room, round),
        _ => {}
    }
    if room >= lanes
        && let Some(found) = at_once(lanes, &round(&(0..lanes).collect::<Vec<_>>()))
    {
        return found;
    }
    let red = bisected(lanes, |count| red(round(&[count]).pop()))?;
    if red == 1 && matches!(round(&[0]).pop(), Some(Verdict::RedMain(_))) {
        return None;
    }
    Some(red - 1)
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

fn among(named: &[usize], room: usize, mut round: impl FnMut(&[usize]) -> Vec<Verdict>) -> Option<usize> {
    let through = |at: usize| named[at] + 1;
    if room + 1 >= named.len() {
        let verdicts = round(&(0..named.len() - 1).map(through).collect::<Vec<_>>());
        for (at, verdict) in verdicts.into_iter().enumerate() {
            if red(Some(verdict))? {
                return Some(named[at]);
            }
        }
        return named.last().copied();
    }
    bisected(named.len(), |at| red(round(&[through(at - 1)]).pop())).map(|red| named[red - 1])
}

fn red(verdict: Option<Verdict>) -> Option<bool> {
    match verdict {
        Some(Verdict::RedMain(_)) => Some(true),
        Some(Verdict::Green) => Some(false),
        _ => None,
    }
}

fn at_once(lanes: usize, verdicts: &[Verdict]) -> Option<Option<usize>> {
    if matches!(verdicts.first(), Some(Verdict::RedMain(_))) {
        return Some(None);
    }
    for (count, verdict) in verdicts.iter().enumerate().skip(1) {
        match verdict {
            Verdict::Green => {}
            Verdict::RedMain(_) => return Some(Some(count - 1)),
            _ => return None,
        }
    }
    Some(Some(lanes - 1))
}

fn bisected(lanes: usize, mut red_at: impl FnMut(usize) -> Option<bool>) -> Option<usize> {
    let (mut green, mut red) = (0, lanes);
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
