use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use crate::platform::push_ledger;
use super::super::{Range, git};
use super::limits;
use north_base::platform::records_mirror::state_home;
use north_base::foundation::exit::NO_VERDICT;
use north_base::foundation::offload::{ONE_LANE_ENV, SLOT_ENV, TIPS_ENV};
use north_base::foundation::carry::CHECK_ENDS;
use north_base::platform::orchestration;

mod own_tree;
mod sweep;
use sweep::{checkout_of, on_this_host};

pub const USAGE: &str = "usage: north land scan test-admission kills [--host NAME] [--remote-north COMMAND] BASE..HEAD\n       north land scan test-admission kills --here BASE..HEAD";
const SHIP_SECONDS: u64 = 900;
const SHOWN: usize = 5;
const BASELINE_FAILED: i32 = 4;
const WRAP_UP: u64 = 45;

struct Lane {
    base: String,
    head: String,
    names: BTreeSet<String>,
    files: Vec<String>,
    sources: Vec<String>,
}

enum End {
    Exited(ExitStatus),
    Caught,
    Capped,
}

pub fn run(directory: &Path, arguments: &[String], out: &mut impl Write) -> u8 {
    let (mut host, mut remote, mut here, mut range, mut bad) = (None, "north".to_owned(), false, None, false);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--here" => here = true,
            "--host" => match words.next() {
                Some(name) => host = Some(name.clone()),
                None => bad = true,
            },
            "--remote-north" => remote = words.next().cloned().unwrap_or_default(),
            value if range.is_none() && !value.starts_with('-') => range = Some(value.to_owned()),
            _ => bad = true,
        }
    }
    let tips = std::env::var(TIPS_ENV).unwrap_or_default();
    if let Some(lanes) = range.as_deref().filter(|_| !bad).and_then(|range| judged_alone(directory, range, &tips)) {
        let _ = writeln!(out, "mutant kill: not run in this probe: each of its {lanes} lane(s) was judged alone against its fork point before it queued, and no two of them touch the same file");
        return 0;
    }
    let judge = |lane: &Lane, out: &mut _| if here { on_this_host(directory, lane, out) } else { offloaded(directory, lane, host.clone(), &remote, out) };
    let Some(code) = range.as_deref().filter(|_| !bad && !remote.is_empty()).and_then(|range| judge_each(directory, range, &tips, here || one_lane(std::env::var(ONE_LANE_ENV).ok().as_deref()), judge, out)) else {
        eprintln!("{USAGE}");
        return 64;
    };
    code
}

fn one_lane(flag: Option<&str>) -> bool {
    flag == Some("1")
}

/// Judges each lane of `range` against its own diff with `judge` and returns the worst verdict, stopping at the first refusal, so one lane's added tests are never paired with the mutants of another lane's diff. `--here` and a range the checks before queueing flag as one lane are one lane; any other range splits at the probe's `tips` when they end at its head, else at each commit, since a lane lands as one commit.
fn judge_each<O: Write>(directory: &Path, range: &str, tips: &str, whole: bool, mut judge: impl FnMut(&Lane, &mut O) -> u8, out: &mut O) -> Option<u8> {
    let mut worst = 0;
    let mut judged = false;
    for part in lane_ranges(directory, range, tips, whole)? {
        let Some(lane) = lane(directory, &part)? else { continue };
        judged = true;
        match judge(&lane, out) {
            0 => {}
            1 => return Some(1),
            code => worst = code,
        }
    }
    if !judged {
        let _ = writeln!(out, "mutant kill: not applicable (the lane adds no Rust test next to a change in its mutated source)");
    }
    Some(worst)
}

fn lane_ranges(directory: &Path, range: &str, tips: &str, whole: bool) -> Option<Vec<String>> {
    let Range { base, head } = Range::parse(range)?;
    if whole {
        return Some(vec![range.to_owned()]);
    }
    let commit = |revision: &str| git::text(directory, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")]).map(|sha| sha.trim().to_owned());
    let (base, head) = (commit(&git::merge_base(directory, base, head))?, commit(head)?);
    let tips: Vec<String> = tips.split_whitespace().map(str::to_owned).collect();
    let ends = if tips.last().and_then(|last| commit(last)).is_some_and(|last| last == head) {
        tips
    } else {
        git::text(directory, &["rev-list", "--reverse", "--first-parent", &format!("{base}..{head}")])?.lines().map(str::to_owned).collect()
    };
    let mut from = base;
    let mut ranges = Vec::new();
    for end in ends {
        ranges.push(format!("{from}..{end}"));
        from = end;
    }
    Some(if ranges.is_empty() { vec![range.to_owned()] } else { ranges })
}

/// The number of lanes in a probe whose lanes, ending at `tips` oldest first, touch pairwise disjoint files; `None` outside a probe or when two lanes share a file, so the merged tree is judged.
fn judged_alone(directory: &Path, range: &str, tips: &str) -> Option<usize> {
    let Range { base, head } = Range::parse(range)?;
    let tips: Vec<&str> = tips.split_whitespace().collect();
    let commit = |revision: &str| git::text(directory, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")]).map(|sha| sha.trim().to_owned());
    if commit(tips.last()?)? != commit(head)? {
        return None;
    }
    let mut from = git::merge_base(directory, base, head);
    let mut touched = BTreeSet::new();
    for tip in &tips {
        let files = git::text(directory, &["diff", "--name-only", "--no-renames", &from, tip])?;
        if !files.lines().all(|file| touched.insert(file.to_owned())) {
            return None;
        }
        from = (*tip).to_owned();
    }
    Some(tips.len())
}

fn lane(directory: &Path, range: &str) -> Option<Option<Lane>> {
    let Range { base, head } = Range::parse(range)?;
    let base = git::merge_base(directory, base, head);
    let head = git::text(directory, &["rev-parse", "--verify", &format!("{head}^{{commit}}")])?.trim().to_owned();
    let found = super::changes(directory, &base, &head);
    let names = super::fresh_names(directory, &base, &found.patch);
    let sources: Vec<String> = git::text(directory, &["config", "--blob", &format!("{base}:.safe-push"), "--get-all", "mutant.source"]).unwrap_or_default().lines().map(str::to_owned).collect();
    let unchanged = git::bytes(directory, &[&["diff", "--quiet", &base, &head, "--"][..], &sources.iter().map(String::as_str).collect::<Vec<_>>()].concat()).is_some();
    let files = found.files.into_iter().filter(|file| file.ends_with(".rs")).collect();
    Some((!names.is_empty() && !unchanged).then_some(Lane { base, head, names, files, sources }))
}

fn input_key(directory: &Path, lane: &Lane, cap: u64) -> Option<String> {
    let tree = |commit: &str| git::text(directory, &["rev-parse", &format!("{commit}^{{tree}}")]).map(|tree| tree.trim().to_owned());
    let text = format!("mutant-kill v1\0{}\0{}\0{cap}", tree(&lane.base)?, tree(&lane.head)?);
    Some(Sha256::digest(text).iter().map(|byte| format!("{byte:02x}")).collect())
}

fn offloaded(directory: &Path, lane: &Lane, host: Option<String>, remote: &str, out: &mut impl Write) -> u8 {
    let origin = git::text(directory, &["remote", "get-url", "origin"]).unwrap_or_default();
    let store = state_home().join("agents/mutants").join(push_ledger::repository(origin.trim())).join("kills");
    let key = input_key(directory, lane, limits::kill_seconds());
    if let Some(stored) = key.as_ref().and_then(|key| fs::read_to_string(store.join(key)).ok()) {
        let (code, verdict) = stored.split_once('\n').unwrap_or((&stored, ""));
        if let Ok(code) = code.parse::<u8>() {
            let _ = writeln!(out, "mutant kill: reusing the verdict for unchanged input {}", &key.as_deref().unwrap_or_default()[..12]);
            let _ = out.write_all(verdict.as_bytes());
            return code;
        }
    }
    let hosts = hosts(directory, host);
    if hosts.is_empty() {
        let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: no build host has room (north capacity offload choose --any) and .safe-push declares no landing.remote-hosts");
        return NO_VERDICT;
    }
    let local = hosts.first().is_some_and(|first| *first == north_base::platform::host::name()) && checkout_of(directory, &lane.head).is_some();
    let (code, verdict) = if local {
        let mut ran = Vec::new();
        let code = on_this_host(directory, lane, &mut ran);
        let _ = out.write_all(&ran);
        (code, verdict_of(&ran))
    } else {
        shipped(directory, lane, &hosts, remote, out)
    };
    if let Some(key) = key.filter(|_| code <= 1 && !verdict.is_empty()) {
        let _ = fs::create_dir_all(&store).and_then(|()| fs::write(store.join(key), format!("{code}\n{verdict}")));
    }
    code
}

fn shipped(directory: &Path, lane: &Lane, hosts: &[String], remote: &str, out: &mut impl Write) -> (u8, String) {
    let cap = match sweep_cap() {
        Ok(cap) => cap,
        Err(why) => return (unavailable(out, &why), String::new()),
    };
    let timeout = seconds_left().unwrap_or(cap + SHIP_SECONDS);
    let command = format!("{remote} land scan test-admission kills --here {}..{}", lane.base, lane.head);
    let mut attempt = None;
    for host in hosts {
        let _ = writeln!(out, "mutant kill: running cargo-mutants over the lane's diff on {host}, capped at {cap} s, for {}", lane.names.iter().cloned().collect::<Vec<_>>().join(", "));
        let ran = Command::new(north_base::platform::north_binary::north_binary())
            .current_dir(directory)
            .args(["capacity", "offload", "run", "--host", host, "--base", &lane.base, "--head", &lane.head, "--timeout", &timeout.to_string(), "--", &command])
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output();
        let Ok(output) = ran else {
            let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: north capacity offload run did not start");
            return (NO_VERDICT, String::new());
        };
        let reached = matches!(output.status.code(), Some(0 | 1));
        let _ = out.write_all(&output.stdout);
        attempt = Some((host, output));
        if reached {
            break;
        }
    }
    let Some((host, output)) = attempt else { return (NO_VERDICT, String::new()) };
    let verdict = verdict_of(&output.stdout);
    let code = match output.status.code() {
        Some(0) if !verdict.is_empty() => 0,
        Some(1) if verdict.contains("safe-push: ✗ mutant kill") => 1,
        _ => unavailable(out, &format!("north capacity offload run on {host} exited {}, so no verdict was reached", output.status)),
    };
    (code, verdict)
}

fn verdict_of(printed: &[u8]) -> String {
    String::from_utf8_lossy(printed).lines().filter(|line| line.starts_with("mutant kill:") || line.starts_with("safe-push: ✗ mutant kill")).map(|line| format!("{line}\n")).collect()
}

fn seconds_left() -> Option<u64> {
    let ends: u64 = std::env::var(CHECK_ENDS).ok()?.parse().ok()?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(ends.saturating_sub(now))
}

fn sweep_cap() -> Result<u64, String> {
    let Some(left) = seconds_left() else { return Ok(limits::kill_seconds()) };
    left.checked_sub(WRAP_UP).filter(|cap| *cap > 0).map(|cap| cap.min(limits::kill_seconds())).ok_or_else(|| format!("its land check has {left} s left, too few to build and judge a mutant"))
}

/// The hosts to try in order: the named one alone, else the one with room, then each declared build host this host reaches.
fn hosts(directory: &Path, named: Option<String>) -> Vec<String> {
    if let Some(named) = named {
        return vec![named];
    }
    let here = north_base::platform::host::name();
    let in_slot = std::env::var_os(SLOT_ENV).is_some_and(|slot| !slot.is_empty());
    let declared = git::text(directory, &["config", "--file", ".safe-push", "--get", "landing.remote-hosts"]).unwrap_or_default();
    fallbacks(in_slot, || choose(directory, &here), &declared, orchestration::read().ok().as_ref(), &here)
}

/// A check inside an offload slot runs on the slot's host, which took it, as coverage does; any other tries the host with room, then each declared host this host reaches, and runs it itself when it reaches none.
fn fallbacks(in_slot: bool, chosen: impl FnOnce() -> Option<String>, declared: &str, cfg: Option<&DocumentMut>, here: &str) -> Vec<String> {
    if in_slot {
        return vec![here.to_owned()];
    }
    let reachable = |host: &str| cfg.is_none_or(|cfg| orchestration::reaches(cfg, here, host));
    let mut hosts: Vec<String> = chosen().into_iter().collect();
    for host in declared.split_whitespace().filter(|host| reachable(host)) {
        if !hosts.iter().any(|known| known == host) {
            hosts.push(host.to_owned());
        }
    }
    if hosts.is_empty() {
        hosts.push(here.to_owned());
    }
    hosts
}

fn choose(directory: &Path, here: &str) -> Option<String> {
    let output = Command::new(north_base::platform::north_binary::north_binary()).current_dir(directory).args(["capacity", "offload", "choose", "--any", "--avoid", here]).stdin(Stdio::null()).stderr(Stdio::inherit()).output().ok()?;
    let host = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !host.is_empty()).then_some(host)
}

struct Tally {
    total: usize,
    cap: u64,
    seconds: u64,
}

fn judge(work: &Path, lane: &Lane, end: &End, tally: Tally, out: &mut impl Write) -> u8 {
    let Tally { total, cap, seconds } = tally;
    let listed = |kind: &str| -> Vec<String> { fs::read_to_string(work.join("mutants.out").join(format!("{kind}.txt"))).unwrap_or_default().lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect() };
    let (mut caught, survived) = (listed("caught"), listed("missed"));
    caught.extend(listed("timeout").into_iter().map(|mutant| format!("{mutant} (the tests time out)")));
    let tested = caught.len() + survived.len() + listed("unviable").len();
    let names = lane.names.iter().cloned().collect::<Vec<_>>().join(", ");
    if let Some(first) = caught.first() {
        let _ = writeln!(out, "mutant kill: ✓ {names} kill {first} ({tested} of {total} mutant(s) of the lane's diff tested, {seconds} s)");
        return 0;
    }
    let shown = survived.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("; ");
    match end {
        End::Exited(status) if status.code() == Some(BASELINE_FAILED) => {
            unavailable(out, &format!("{names} fail on the lane's own unmutated tree, so no mutant can be judged; runner output ends:\n{}", tail(work)))
        }
        End::Exited(status) if status.code().is_none_or(|code| ![0, 2, 3].contains(&code)) => unavailable(out, &format!("cargo mutants exited {status}; its output ends:\n{}", tail(work))),
        End::Capped if tested < total => unavailable(out, &format!("the {cap} s cap passed after {tested} of {total} mutant(s), none killed yet, so the lane is not judged")),
        _ if survived.is_empty() => {
            let _ = writeln!(out, "mutant kill: not applicable (none of the {total} mutant(s) of the lane's diff builds)");
            0
        }
        _ => {
            let _ = writeln!(out, "safe-push: ✗ mutant kill: {names} survive all {} mutant(s) of the lane's diff ({tested} tested), so the added tests guard none of the change: {shown}; make a test fail when the changed code is broken.", survived.len());
            1
        }
    }
}

fn tail(work: &Path) -> String {
    let text = fs::read_to_string(work.join("runner.log")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

fn unavailable(out: &mut impl Write, reason: &str) -> u8 {
    let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: {reason}");
    NO_VERDICT
}

#[cfg(test)]
#[path = "kills_tests.rs"]
mod tests;
