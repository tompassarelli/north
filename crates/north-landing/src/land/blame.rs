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

pub fn ask(repo: &Path, log: &Path, base: &str, head: &str, lanes: &[String], tips: &[String], room: usize) -> Suspects {
    let mut command = Command::new(follow::program());
    command.args(["graph", "ask", "blame", "--log"]).arg(log).current_dir(repo).stdin(Stdio::null());
    for (at, (lane, tip)) in lanes.iter().zip(tips).enumerate() {
        command.arg(format!("{lane}={}..{tip}", if at == 0 { base } else { &tips[at - 1] }));
    }
    let asked = north_base::platform::process::run_command_within(command, None, ASK_LIMIT);
    let suspects = asked.as_ref().map_or(Suspects::Batch, |output| self::suspects(lanes, output.status.code(), &String::from_utf8_lossy(&output.stdout)));
    match &suspects {
        Suspects::Named(named) if named.len() == 1 => println!("north land: the change graph blames {} alone for the red after-landing run of {head}, so it is reverted with no bisect round", lanes[named[0]]),
        Suspects::Named(named) => println!("north land: the change graph blames {} of the {} lanes of {head}, so the culprit is bisected among {}", named.len(), lanes.len(), named.iter().map(|&at| lanes[at].as_str()).collect::<Vec<_>>().join(", ")),
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
