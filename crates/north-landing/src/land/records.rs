use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::{Command, Stdio};

use super::after::follow;
use super::candidate::git;
use super::incidents;
use super::lander::Lander;
use super::queue::{self, Ticket};
use super::telemetry::{self, Stages};

pub struct Candidate<'a> {
    pub head: &'a str,
    pub batch_size: usize,
    pub stages: &'a Stages,
}

pub enum Decision<'a> {
    Landed { sha: &'a str },
    Refused { verdict: &'a str },
}

fn class(verdict: &str) -> Option<&'static str> {
    match verdict.split_whitespace().next() {
        Some("failed") => Some("failed"),
        Some("conflict") => Some("conflict"),
        _ => None,
    }
}

fn flag(arguments: &mut Vec<String>, name: &str, value: impl ToString) {
    arguments.push(name.to_owned());
    arguments.push(value.to_string());
}

pub fn arguments(path: &Path, ticket: &Ticket, candidate: &Candidate, tree: &str, decision: &Decision, at: f64) -> Option<Vec<String>> {
    let queued = queue::queued_micros(path);
    if queued == u128::MAX {
        return None;
    }
    let lane = ticket.root.file_name()?.to_string_lossy().into_owned();
    let mut arguments = vec!["land-train".to_owned(), "record".to_owned()];
    flag(&mut arguments, "--repo", telemetry::repository(&ticket.origin));
    flag(&mut arguments, "--lane", lane);
    flag(&mut arguments, "--branch", &ticket.branch);
    flag(&mut arguments, "--tip", &ticket.head);
    flag(&mut arguments, "--queued-at", queued as f64 / 1e6);
    if let Some(first) = ticket.first_commit_secs {
        flag(&mut arguments, "--first-commit-at", first);
    }
    if !candidate.head.is_empty() {
        flag(&mut arguments, "--candidate", candidate.head);
        flag(&mut arguments, "--tree", tree);
    }
    flag(&mut arguments, "--batch-size", candidate.batch_size);
    flag(&mut arguments, "--bisect-depth", candidate.stages.bisect_depth);
    flag(&mut arguments, "--attempts", candidate.stages.attempts);
    if let Some((started, finished)) = candidate.stages.check_window() {
        flag(&mut arguments, "--check-started-at", started);
        flag(&mut arguments, "--check-finished-at", finished);
    }
    match decision {
        Decision::Landed { sha } => {
            flag(&mut arguments, "--verdict", "landed");
            flag(&mut arguments, "--landed-at", at);
            flag(&mut arguments, "--landed-sha", sha);
        }
        Decision::Refused { verdict } => {
            flag(&mut arguments, "--verdict", class(verdict)?);
            flag(&mut arguments, "--detail", verdict);
        }
    }
    Some(arguments)
}

pub(super) fn spawn(queue: &Path, arguments: &[String]) -> Result<(), String> {
    let logs = queue.join("logs");
    fs::create_dir_all(&logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = OpenOptions::new().create(true).append(true).open(logs.join("landing-records.log")).map_err(|error| error.to_string())?;
    let errors = log.try_clone().map_err(|error| error.to_string())?;
    let mut child = Command::new(follow::program())
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .spawn()
        .map_err(|error| format!("{} {}: {error}", follow::program(), arguments.get(..2).unwrap_or(arguments).join(" ")))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn write(lander: &Lander, path: &Path, ticket: &Ticket, candidate: &Candidate, decision: &Decision) {
    let tree = if candidate.head.is_empty() { String::new() } else { git(&lander.worktree, &["rev-parse", &format!("{}^{{tree}}", candidate.head)]).unwrap_or_default() };
    if let Decision::Refused { verdict } = decision {
        incidents::record(&lander.queue, path, ticket, incidents::decided(path, ticket, verdict));
    }
    let Some(arguments) = arguments(path, ticket, candidate, &tree, decision, telemetry::epoch()) else { return };
    if let Err(error) = spawn(&lander.queue, &arguments) {
        println!("north land: could not record {}: {error}", ticket.branch);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ticket() -> Ticket {
        Ticket {
            origin: "https://github.com/tompassarelli/north.git".into(),
            root: PathBuf::from("/lanes/s9-landing-records"),
            host: "laptop".into(),
            branch: "s9-landing-records".into(),
            head: "f".repeat(40),
            session: None,
            queued_at: "2026-11-04T03:00:00Z".into(),
            first_commit_secs: Some(1_793_800_000),
            several_commits: None,
            red_fix: None,
            verdict: None,
            keep_lane: false,
        }
    }

    fn words(arguments: &[String]) -> String {
        arguments.join(" ")
    }

    #[test]
    fn a_landed_lane_carries_every_time_the_report_splits_on() {
        let mut stages = Stages::start();
        stages.mark("check");
        stages.attempts = 2;
        stages.bisect_depth = 1;
        let path = PathBuf::from("/queue/1793800600000000-s9-landing-records.json");
        let candidate = Candidate { head: "abc", batch_size: 3, stages: &stages };
        let text = words(&arguments(&path, &ticket(), &candidate, "tree1", &Decision::Landed { sha: "abc" }, 1_793_801_000.0).unwrap());
        for part in ["land-train record", "--repo north", "--lane s9-landing-records", "--tip ffff", "--queued-at 1793800600", "--first-commit-at 1793800000", "--candidate abc --tree tree1", "--batch-size 3 --bisect-depth 1 --attempts 2", "--check-started-at", "--check-finished-at", "--verdict landed --landed-at 1793801000 --landed-sha abc"] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
    }

    #[test]
    fn only_a_conflict_or_a_failed_check_is_a_refusal_and_a_ticket_without_a_queue_time_is_not_recorded() {
        let stages = Stages::empty();
        let path = PathBuf::from("/queue/1793800600000000-x.json");
        let none = Candidate { head: "", batch_size: 0, stages: &stages };
        let refused = |verdict: &str| arguments(&path, &ticket(), &none, "", &Decision::Refused { verdict }, 0.0).map(|arguments| words(&arguments));
        assert!(refused("conflict shared.txt").unwrap().contains("--verdict conflict --detail conflict shared.txt"));
        assert!(refused("failed https://example/runs/1").unwrap().contains("--verdict failed"));
        assert!(!refused("failed x").unwrap().contains("--candidate"));
        assert_eq!(refused("landed abc"), None);
        assert_eq!(arguments(Path::new("/queue/unnamed.json"), &ticket(), &none, "", &Decision::Landed { sha: "a" }, 0.0), None);
    }
}
