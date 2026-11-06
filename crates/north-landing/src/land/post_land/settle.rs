use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;
use north_base::foundation::offload::OWNER_ENV;

use super::{ALARMED, EPISODE, Checked, Landed, Run, check, deadline, failing, ledger_path, record, steps};
use crate::landing::red_check::Failure;
use crate::platform::push_ledger;
use crate::land::after::follow;
use crate::land::candidate::git;
use crate::land::checks::Verdict;
use crate::land::lander::Lander;
use crate::land::red_episode::Episode;
use crate::land::setting;

pub(crate) fn rerun(lander: &Lander, base: &str, tree: &str, failing: &[String]) -> Option<BTreeSet<Failure>> {
    let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let landed = Landed { repo: lander.repo.clone(), repository: push_ledger::repository(&origin), base: base.to_owned(), head: tree.to_owned(), heads: Vec::new(), lanes: Vec::new(), tips: Vec::new() };
    let mut runner = vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--dry".to_owned(), "1".to_owned()];
    for step in failing {
        runner.extend(["--command".to_owned(), step.clone()]);
    }
    let logs = lander.queue.join("logs");
    let name = format!("sheriff-{}", &tree[..tree.len().min(12)]);
    let run = Run {
        runner,
        worktree: lander.queue.join("sheriff-probe"),
        log: logs.join(format!("{name}.log")),
        outcome: logs.join(format!("{name}.outcome")),
        ledger: PathBuf::new(),
        offload_owner: lander.offload_owner.clone(),
        capacity: setting::landing(&lander.repo, "refs/remotes/origin/main", "landing.capacity"),
        deadline: deadline(&lander.repo, &ledger_path(), &landed.repository, tree),
        host: None,
        cancel: Arc::default(),
        adoptable: false,
    };
    rerun_result(&check(&landed, tree, &run).verdict, &run.outcome, failing)
}

pub(super) fn rerun_result(verdict: &Verdict, outcome: &Path, requested: &[String]) -> Option<BTreeSet<Failure>> {
    let requested: BTreeSet<Failure> = requested.iter().map(|step| Failure::Step(step.clone())).collect();
    match verdict {
        Verdict::Green => Some(BTreeSet::new()),
        Verdict::RedMain(_) => Some(steps(outcome)).filter(|failed| failed.is_subset(&requested)),
        _ => None,
    }
}

pub fn settle(landed: &Landed, run: &Run, alarm: impl Fn(&str, &str) -> bool) -> Verdict {
    let started = Instant::now();
    let checked = check(landed, &landed.head, run);
    settle_from(landed, run, started, checked, alarm, |command| offload_rerun(landed, run, command))
}

pub(super) fn settle_from(landed: &Landed, run: &Run, started: Instant, mut checked: Checked, alarm: impl Fn(&str, &str) -> bool, rerun: impl Fn(&str) -> String) -> Verdict {
    if matches!(checked.verdict, Verdict::Transient(_)) && !checked.expired && !run.superseded() {
        println!("north land: the post-land checks of {} reached no verdict; rerunning them once", landed.head);
        checked = check(landed, &landed.head, run);
    }
    let Checked { verdict, expired } = checked;
    record(run, landed, &verdict, expired, started.elapsed().as_secs());
    if verdict == Verdict::Green {
        rerun_red(landed, run, rerun);
    }
    announce(landed, run, &verdict, alarm);
    verdict
}

pub(super) fn episode_marker(run: &Run, landed: &Landed) -> PathBuf {
    run.ledger.with_file_name(format!("{EPISODE}-{}", landed.repository.replace('/', "_")))
}

pub(super) fn rerun_red(landed: &Landed, run: &Run, rerun: impl Fn(&str) -> String) {
    let Some(episode) = Episode::read(&episode_marker(run, landed)) else { return };
    if episode.ended_by(&fs::read_to_string(&run.log).unwrap_or_default(), &landed.repo, &landed.head) {
        return;
    }
    let Some(command) = episode.rerun_command() else { return };
    let names: Vec<&str> = episode.named().into_iter().map(Failure::label).collect();
    println!("north land: main has been red since {}, so the after-landing run of {} reruns {}", episode.head, landed.head, names.join(", "));
    let log = rerun(&command);
    if let Err(error) = OpenOptions::new().append(true).open(&run.log).and_then(|mut file| file.write_all(log.as_bytes())) {
        eprintln!("north land: could not add the rerun of main's red tests to {}: {error}", run.log.display());
    }
}

pub(super) fn offload_rerun(landed: &Landed, run: &Run, command: &str) -> String {
    let host = run.host.clone().unwrap_or_else(north_base::platform::host::name);
    let timeout = run.deadline.as_secs().to_string();
    let ran = Command::new(follow::program())
        .args(["capacity", "offload", "run", "--host", &host, "--base", &landed.head, "--head", &landed.head, "--timeout", &timeout, "--", command])
        .current_dir(&landed.repo)
        .env(OWNER_ENV, &run.offload_owner)
        .stdin(Stdio::null())
        .output();
    match ran {
        Ok(output) => format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        Err(error) => {
            eprintln!("north land: could not rerun main's red tests on {host}: {error}");
            String::new()
        }
    }
}

pub(super) fn red_message(landed: &Landed, run: &Run, detail: &str) -> String {
    let log = &run.log;
    let failing: Vec<String> = failing(log, &run.outcome).iter().map(|failure| failure.label().to_owned()).collect();
    let failing = if failing.is_empty() { detail.to_owned() } else { failing.join(", ") };
    format!("north land: main is red in {} at {}: failing {failing}; suspect commit {} (landed lanes: {}); log: {}; the sheriff acts first, this message is for when it cannot", landed.repository, landed.head, landed.head, landed.lanes.join(", "), log.display())
}

pub(super) fn green_message(landed: &Landed) -> String {
    format!("north land: main is green again in {} at {}", landed.repository, landed.head)
}

pub(super) fn announce(landed: &Landed, run: &Run, verdict: &Verdict, alarm: impl Fn(&str, &str) -> bool) {
    let marker = episode_marker(run, landed);
    let episode = Episode::read(&marker);
    let log = fs::read_to_string(&run.log).unwrap_or_default();
    match verdict {
        Verdict::RedMain(detail) if episode.is_none() => {
            if alarm(ALARMED, &red_message(landed, run, detail)) {
                let _ = Episode::opened(&landed.head, &log, failing(&run.log, &run.outcome)).write(&marker);
            } else {
                eprintln!("north land: could not alarm {ALARMED} about red main {}", landed.head);
            }
        }
        Verdict::RedMain(_) => {
            if let Some(mut episode) = episode {
                episode.failing.extend(failing(&run.log, &run.outcome));
                let _ = episode.write(&marker);
            }
        }
        Verdict::Green if episode.as_ref().is_some_and(|episode| !episode.ended_by(&log, &landed.repo, &landed.head)) => {
            println!("north land: the green after-landing run of {} did not rerun the checks that turned main red, so main stays red", landed.head);
        }
        Verdict::Green if episode.is_some() => {
            if alarm(ALARMED, &green_message(landed)) {
                let _ = fs::remove_file(&marker);
            } else {
                eprintln!("north land: could not tell {ALARMED} that main is green again at {}", landed.head);
            }
        }
        _ => {}
    }
}
