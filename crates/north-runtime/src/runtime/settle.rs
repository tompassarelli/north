use super::canary::{self, Verdict};
use super::deploy;
use super::failure::{Failure, failed};
use super::layout::{self, Layout};
use super::probe::{self, Shell};
use super::update;
use crate::land_offload;
use crate::platform::{host, process};
use rustix::fs::{FlockOperation, flock};
use serde_json::json;
use std::fs::{self, File};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SEEN_KEPT: usize = 50;

struct Landing {
    sha: String,
    subject: String,
}

pub(super) fn settle(layout: &Layout) -> Result<(), Failure> {
    fs::create_dir_all(&layout.state)?;
    let lock = File::create(layout.state.join("runtime-settle.lock"))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        println!("north runtime settle already running");
        return Ok(());
    }
    if let Err(failure) = update::update(layout) {
        eprintln!("north runtime settle: {}: {}", host::name(), failure.message);
    }
    let checkout = update::checkout();
    let destinations = land_offload::other_destinations();
    let here = fs::read_link(layout.current()).ok().and_then(|link| layout::linked_sha(&link).map(str::to_owned));
    let mut hosts = vec![Shell::Here(host::name())];
    hosts.extend(destinations.iter().cloned().map(Shell::Ssh));
    if let Some(here) = &here {
        let runtime = layout.runtime(here);
        for host in &hosts[1..] {
            let behind = probe::live_runtime(host).is_none_or(|remote| remote != *here && ancestor(&checkout, &remote, here));
            if !behind {
                continue;
            }
            if let Err(failure) = deploy::ship_runtime(host.name(), here, &runtime) {
                eprintln!("north runtime settle: {}", failure.message);
            }
        }
    }
    let landing = due_landing(layout, &checkout)?;
    let live = |runtime: &str| landing.as_ref().is_none_or(|landing| runtime == landing.sha || ancestor(&checkout, &landing.sha, runtime));
    let mut records = Vec::new();
    let mut failing = Vec::new();
    let mut broken = Vec::new();
    let moved_on = landing.as_ref().is_some_and(|landing| origin_head(&checkout).is_some_and(|head| head != landing.sha));
    for host in &hosts {
        let facts = probed(layout, &checkout, host);
        let problems = probe::problems(&facts, live);
        println!("north runtime settle: {}: {}", facts.host, if problems.is_empty() { "live and healthy".to_owned() } else { problems.join("; ") });
        if !problems.is_empty() {
            failing.push(format!("{}: {}", facts.host, problems.join("; ")));
            broken.extend(landing_breakage(&facts.host, &problems, moved_on));
        }
        records.push(probe::record(&facts, &problems));
    }
    let report = json!({"checkedAt": now(), "landing": landing.as_ref().map(|landing| &landing.sha), "hosts": records});
    fs::write(layout.state.join("runtime-settle.json"), format!("{report:#}\n"))?;
    if let Some(landing) = landing.filter(|landing| !judged(layout, &landing.sha)) {
        fs::write(layout.state.join("runtime-settle-judged"), &landing.sha)?;
        if !broken.is_empty() {
            alarm(&checkout, &landing, &broken);
        }
    }
    match failing.is_empty() {
        true => Ok(()),
        false => Err(failed(1, format!("not live and healthy everywhere: {}", failing.join(" | ")))),
    }
}

fn probed(layout: &Layout, checkout: &Path, host: &Shell) -> probe::Facts {
    let facts = probe::facts(host);
    let judged = match &facts.runtime {
        Some(runtime) if facts.unreachable.is_none() => canary::check(layout, &facts.host, runtime, &canary::Live { layout, checkout, host }),
        _ => Verdict::Skipped,
    };
    match judged {
        Verdict::RolledBack => probe::facts(host),
        _ => facts,
    }
}

fn due_landing(layout: &Layout, checkout: &Path) -> Result<Option<Landing>, Failure> {
    let seen_path = layout.state.join("runtime-settle-seen");
    let mut seen: Vec<(u64, String)> = fs::read_to_string(&seen_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(' ').and_then(|(at, sha)| Some((at.parse().ok()?, sha.to_owned()))))
        .collect();
    let head = origin_head(checkout);
    if let Some(head) = head.filter(|head| seen.last().is_none_or(|(_, last)| last != head)) {
        seen.push((now(), head));
        let kept = &seen[seen.len().saturating_sub(SEEN_KEPT)..];
        fs::write(&seen_path, kept.iter().map(|(at, sha)| format!("{at} {sha}\n")).collect::<String>())?;
    }
    let deadline = probe::default_number("landing_live_deadline_sec") as u64;
    let Some((_, sha)) = seen.iter().rev().find(|(at, _)| *at + deadline <= now()) else {
        return Ok(None);
    };
    let subject = process::run("git", Some(checkout), &["log", "-1", "--format=%s", sha]).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
    Ok(Some(Landing { sha: sha.clone(), subject }))
}

fn landing_breakage(host: &str, problems: &[String], moved_on: bool) -> Option<String> {
    let own: Vec<&str> = problems.iter().map(String::as_str).filter(|problem| !(moved_on && problem.starts_with(probe::LAGS))).collect();
    (!own.is_empty()).then(|| format!("{host}: {}", own.join("; ")))
}

fn origin_head(checkout: &Path) -> Option<String> {
    process::run("git", Some(checkout), &["rev-parse", "--verify", "origin/main^{commit}"]).ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|sha| layout::is_sha(sha))
}

fn judged(layout: &Layout, sha: &str) -> bool {
    fs::read_to_string(layout.state.join("runtime-settle-judged")).is_ok_and(|judged| judged.trim() == sha)
}

fn ancestor(checkout: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(checkout), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn issue(subject: &str) -> Option<&str> {
    let (_, tail) = subject.rsplit_once("(north#")?;
    let number = tail.strip_suffix(')')?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(number)
}

fn alarm(checkout: &Path, landing: &Landing, failing: &[String]) {
    let short = layout::short(&landing.sha);
    let text = format!("north landing {short} is not live and healthy on every host {}s after it landed: {}", probe::default_number("landing_live_deadline_sec"), failing.join(" | "));
    if let Some(number) = issue(&landing.subject) {
        let reopened = Command::new("gh").current_dir(checkout).args(["issue", "reopen", number]).status();
        let commented = Command::new("gh").current_dir(checkout).args(["issue", "comment", number, "--body", &text]).status();
        if !(reopened.is_ok_and(|status| status.success()) || commented.is_ok_and(|status| status.success())) {
            eprintln!("north runtime settle: could not reopen or comment on north#{number}");
        }
    }
    println!("north runtime settle: {text}; north watch raises it to the proxy and the landing lane's lead");
}

#[cfg(test)]
mod tests {
    use super::{issue, landing_breakage};

    #[test]
    fn a_landing_names_its_issue_only_in_a_trailing_north_reference() {
        assert_eq!(issue("Break the runtime-follow livelock (north#308)"), Some("308"));
        assert_eq!(issue("Admit heavy work past load"), None);
        assert_eq!(issue("Fix (north#) parsing"), None);
        assert_eq!(issue("Merge (north#12) follow-up"), None);
    }

    #[test]
    fn a_runtime_lagging_a_newer_main_is_not_this_landings_breakage() {
        let lag = vec!["live runtime 1234abcd predates the landing".to_owned()];
        assert_eq!(landing_breakage("nexus", &lag, true), None);
        assert!(landing_breakage("nexus", &lag, false).is_some_and(|text| text.contains("predates the landing")));
        let mixed = vec![lag[0].clone(), "north-runtime-follow.service is failed".to_owned()];
        assert_eq!(landing_breakage("nexus", &mixed, true).as_deref(), Some("nexus: north-runtime-follow.service is failed"));
    }
}
