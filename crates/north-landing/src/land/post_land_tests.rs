use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use super::{Landed, Run, settle};
use super::execute::{apart, judged};
use super::settle::announce;
use crate::land::checks::Verdict;
use crate::landing::red_check::Failure;

const SWITCHED: &str = "thread 'main' panicked at crates/north-base/src/platform/harness.rs:16:\nnorth: runtime 3c5a1869 has no tree and the checkout ~/code/north/main is at a3ecc7a51; run north runtime update\n";

fn run() -> Run {
    Run { runner: Vec::new(), worktree: PathBuf::new(), log: PathBuf::from("post-land.log"), outcome: PathBuf::from("absent"), ledger: PathBuf::new(), store: PathBuf::new(), offload_owner: String::new(), capacity: String::new(), deadline: Duration::from_secs(60), host: None, cancel: Default::default(), adoptable: false }
}

#[test]
fn a_post_land_run_is_red_main_only_when_a_check_recorded_a_failing_verdict_whatever_the_log_says() {
    for code in [126, 127] {
        let verdict = judged(ExitStatus::from_raw(code << 8), &run());
        assert!(matches!(verdict, Verdict::Transient(_)), "exit {code} became {verdict:?}");
    }
    let scratch = tempfile::tempdir().unwrap();
    let mut switched = run();
    switched.log = scratch.path().join("post-land.log");
    switched.outcome = scratch.path().join("post-land.outcome");
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "a run that failed before any check reached a verdict is not red main");
    std::fs::write(&switched.log, "error: a message no list of infrastructure signatures has seen\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "an unseen infrastructure message reaches no verdict");
    std::fs::write(&switched.log, SWITCHED).unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(101 << 8), &switched), Verdict::Transient(_)), "a runtime switch under the check reaches no verdict");
    std::fs::write(&switched.log, "fatal: '/home/tom/.cache/land-offload/code-north-main-4' is a missing but already registered worktree;\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(128 << 8), &switched), Verdict::Transient(_)), "an evicted build slot reaches no verdict");
    std::fs::write(&switched.log, "bash: line 1: north: command not found\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "a missing tool reaches no verdict");
    std::fs::write(&switched.log, "test land::x ... FAILED\n").unwrap();
    std::fs::write(&switched.outcome, "event\tland check\tfailed\tcargo clippy; log: /x.log\nfailed-step\tcargo clippy\t/x.log\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(101 << 8), &switched), Verdict::RedMain(_)), "a check that recorded its failing verdict is red main");
    assert_eq!(super::failing(&switched.log, &switched.outcome), std::collections::BTreeSet::from([Failure::Cargo("land::x".to_owned()), Failure::Step("cargo clippy".to_owned())]));
    assert_eq!(judged(ExitStatus::from_raw(0), &run()), Verdict::Green);

    let cancelled = |sender: Option<&str>| {
        let cancelled = run();
        if let Some(sender) = sender {
            cancelled.cancel.request(sender);
        }
        match super::superseded(&landed(&["b"]), &cancelled) {
            Verdict::Transient(detail) => detail,
            other => panic!("a cancelled run reached {other:?}"),
        }
    };
    assert!(cancelled(Some("9999 landed past the lander")).starts_with("9999 landed past the lander changed the check lines"), "the cancel record names the landing that cancelled the run");
    assert!(cancelled(None).contains("whose name the lander did not record"), "a run no lander named as its canceller says so rather than blaming a nameless landing");
}

fn landed(heads: &[&str]) -> Landed {
    let heads: Vec<String> = heads.iter().map(|head| (*head).to_owned()).collect();
    Landed { repo: PathBuf::new(), repository: "owner/repo".to_owned(), base: "base".to_owned(), head: heads.last().cloned().unwrap_or_default(), heads, lanes: vec!["lane-a".to_owned(), "lane-b".to_owned()], tips: Vec::new(), ran: Vec::new() }
}

#[test]
fn an_after_landing_run_deadline_is_three_times_the_p90_of_its_repository_s_recorded_runs_and_doubles_after_an_expiry() {
    let scratch = tempfile::tempdir().unwrap();
    let ledger = scratch.path().join("post-land.tsv");
    let row = |repository: &str, class: &str, seconds: u64, covering: &str| format!("1\t{repository}\thead\t{class}\t\tlane\t{seconds}\t{covering}\n");
    let mut rows: String = (1..=10).map(|run| row("owner/repo", if run == 4 { "red-main" } else { "green" }, run * 20, "")).collect();
    rows.push_str(&row("owner/repo", "green", 5_000, "newer"));
    rows.push_str(&row("owner/other", "green", 5_000, ""));
    std::fs::write(&ledger, &rows).unwrap();
    let measured = super::measured_deadline(&ledger, "owner/repo");
    assert_eq!(measured.seconds, 540, "p90 180 s of the repository's own uncovered runs, times 3: {measured}");
    rows.push_str(&row("owner/repo", "transient", 540, ""));
    std::fs::write(&ledger, &rows).unwrap();
    assert_eq!(super::measured_deadline(&ledger, "owner/repo").seconds, 1_080, "a run that expired doubles the next deadline");
    rows.push_str(&row("owner/repo", "green", 100, ""));
    std::fs::write(&ledger, &rows).unwrap();
    assert_eq!(super::measured_deadline(&ledger, "owner/repo").seconds, 540, "a later run that reached a verdict returns the deadline to its measure");
    assert_eq!(super::measured_deadline(&scratch.path().join("absent.tsv"), "owner/repo").seconds, crate::landing::check_timings::CEILING, "an unmeasured repository gets the ceiling");
}

fn committed_repo(directory: &std::path::Path) -> String {
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
        assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(directory.join("README"), "x\n").unwrap();
    git(&["add", "README"]);
    git(&["commit", "-q", "-m", "start"]);
    git(&["rev-parse", "HEAD"])
}

fn a_check_that_a_runtime_switch_killed_is_rerun_once_and_neither_alarms_nor_records_a_red_main() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let head = committed_repo(&repo);
    let counter = scratch.path().join("ran");
    let script = format!("if [ -e {counter} ]; then exit 0; fi; : > {counter}; printf '%s\\n' \"north: runtime 3c5a1869 has no tree and the checkout /x is at a3ecc7a51; run north runtime update\"; exit 101", counter = counter.display());
    let mut switching = run();
    switching.runner = vec!["sh".to_owned(), "-c".to_owned(), script, "check".to_owned()];
    switching.worktree = scratch.path().join("post-land");
    switching.log = scratch.path().join("post-land.log");
    switching.outcome = scratch.path().join("post-land.outcome");
    switching.ledger = scratch.path().join("post-land.tsv");
    switching.store = scratch.path().join("ledger.db");
    std::fs::create_dir_all(switching.worktree.join("target/debug")).unwrap();
    std::fs::write(switching.worktree.join("target/debug/half-removed"), "x").unwrap();
    let mut landed = landed(&[&head]);
    landed.repo = repo;
    landed.head = head;
    let alarms = std::cell::RefCell::new(Vec::new());
    let verdict = settle(&landed, &switching, |_, message| {
        alarms.borrow_mut().push(message.to_owned());
        true
    });
    assert_eq!(verdict, Verdict::Green);
    assert!(alarms.borrow().is_empty(), "{:?}", alarms.borrow());
    assert!(counter.exists());
    assert!(!switching.worktree.exists(), "the run removes a worktree an earlier run left behind");
}

const RED: &str = "safe-push: the Rust checks cover -p north -p north-base\n        FAIL [   0.006s] ( 97/668) north-base error::tests::drill\n    test error::tests::drill ... FAILED\n";
const NARROW: &str = "safe-push: the Rust checks cover -p north -p north-safe-push\n        PASS [   0.633s] ( 21/140) north::it land_checks::other\n";
const FULL: &str = "safe-push: the Rust checks cover -p north -p north-base\n        PASS [   0.006s] ( 97/668) north-base error::tests::drill\n";
const WHOLE: &str = "safe-push: the Rust checks cover --workspace\n        PASS [   0.633s] ( 21/638) north::it land_checks::other\n";

#[test]
fn a_red_main_episode_messages_the_coordinator_once_with_the_suspect_and_ends_only_on_a_green_run_that_reran_its_failing_tests_or_every_test_of_the_workspace_at_a_descendant_or_on_the_sheriffs_revert() {
    a_check_that_a_runtime_switch_killed_is_rerun_once_and_neither_alarms_nor_records_a_red_main();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    committed_repo(&repo);
    let commit = |message: &str| {
        let git = |arguments: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
        assert!(git(&["commit", "-q", "--allow-empty", "-m", message]).status.success());
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_owned()
    };
    let mut episode = run();
    episode.log = scratch.path().join("post-land.log");
    episode.outcome = scratch.path().join("post-land.outcome");
    episode.ledger = scratch.path().join("post-land.tsv");
    episode.store = scratch.path().join("ledger.db");
    let sent = std::cell::RefCell::new(Vec::new());
    let alarm = |session: &str, message: &str| {
        sent.borrow_mut().push((session.to_owned(), message.to_owned()));
        true
    };
    let at = |head: &str, log: &str, verdict: Verdict| {
        let mut landed = landed(&[head]);
        landed.repo.clone_from(&repo);
        std::fs::write(&episode.log, log).unwrap();
        announce(&landed, &episode, &verdict, alarm);
    };
    let marker = scratch.path().join("post-land-red-owner_repo");
    let red = || Verdict::RedMain("exit 100".to_owned());
    let green_again = |at: usize| sent.borrow()[at].1.contains("main is green again") && !marker.exists();

    let culprit = commit("drill");
    std::fs::write(&episode.outcome, "failed-step\tcargo nextest run\t/x.log\n").unwrap();
    at(&culprit, RED, red());
    at(&commit("also red"), RED, red());
    assert_eq!(sent.borrow().len(), 1, "a second red verdict of the episode is silent: {:?}", sent.borrow());
    let (session, message) = sent.borrow()[0].clone();
    assert_eq!(session, "coordinator");
    assert!(message.contains("error::tests::drill") && message.contains(&format!("suspect commit {culprit}")) && message.contains(&episode.log.display().to_string()) && message.contains("lane-a, lane-b"), "{message}");
    at(&commit("docs"), NARROW, Verdict::Green);
    assert_eq!(sent.borrow().len(), 1, "a green run that skipped the failing test sends no green message: {:?}", sent.borrow());
    assert!(marker.exists(), "a narrower green keeps the red-main marker");
    at(&commit("fix"), FULL, Verdict::Green);
    at(&commit("more docs"), NARROW, Verdict::Green);
    assert_eq!(sent.borrow().len(), 2, "a green run over the failing test sends one green message: {:?}", sent.borrow());
    assert!(green_again(1), "{:?}", sent.borrow());

    let culprit = commit("drill again");
    at(&culprit, RED, red());
    at(&commit("docs"), NARROW, Verdict::Green);
    assert_eq!(sent.borrow().len(), 3, "a new episode messages again: {:?}", sent.borrow());
    at(&commit(&format!("Revert drill again\n\nSheriff-Reverts: {culprit}")), NARROW, Verdict::Green);
    at(&commit("docs"), NARROW, Verdict::Green);
    assert_eq!(sent.borrow().len(), 4, "the sheriff's revert landing green sends exactly one green message: {:?}", sent.borrow());
    assert!(green_again(3), "{:?}", sent.borrow());

    let culprit = commit("drill once more");
    at(&culprit, RED, red());
    let selected = |head: &str, every: bool| std::fs::write(&episode.outcome, format!("selected\tbase\t{head}\t{{\"tests\":[],\"added\":[],\"modules\":[],\"every\":{every}}}\n")).unwrap();
    let side = String::from_utf8_lossy(&std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "commit-tree", "-p", &format!("{culprit}^"), "-m", "side", "HEAD^{tree}"]).output().unwrap().stdout).trim().to_owned();
    let between = commit("docs");
    let still = commit("still red");
    at(&still, RED, red());
    for (head, log, every) in [(commit("docs"), NARROW, true), (commit("docs"), WHOLE, false), (side, WHOLE, true), (still, WHOLE, true), (between, WHOLE, true)] {
        selected(&head, every);
        at(&head, log, Verdict::Green);
    }
    assert!(marker.exists() && sent.borrow().len() == 5, "only every test of the whole workspace at a descendant ends an episode by its scope: {:?}", sent.borrow());
    let whole = commit("docs");
    let harness = crate::land::red_episode::Episode { head: culprit, scope: Default::default(), failing: std::collections::BTreeSet::from([Failure::Harness("harness/a.test.sh".to_owned())]), reds: Default::default() };
    assert!(!harness.ended_by(WHOLE, true, &repo, &whole), "a harness failure still needs a pass by name");
    selected(&whole, true);
    at(&whole, WHOLE, Verdict::Green);
    assert!(sent.borrow().len() == 6 && green_again(5), "{:?}", sent.borrow());

    {
        let scratch = tempfile::tempdir().unwrap();
        let repo = scratch.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        committed_repo(&repo);
        let git = |arguments: &[&str]| String::from_utf8_lossy(&std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap().stdout).trim().to_owned();
        let land_checks = |text: &str, message: &str| {
            std::fs::write(repo.join(".safe-push"), text).unwrap();
            git(&["add", ".safe-push"]);
            git(&["commit", "-q", "-m", message]);
            git(&["rev-parse", "HEAD"])
        };
        let red = land_checks("[land]\n\tcheck = src/ :: cargo test kept\n\tcheck = \"src/store/ :: cargo test -E 'test(/old_state/)'\"\n", "add the old-state check");
        let head = land_checks("[land]\n\tcheck = src/ :: cargo test kept\n\t# check = \"src/store/ :: cargo test -E 'test(/old_state/)'\"\n[mutant]\n\tcheck = cargo test -E 'test(/old_state/)'\n", "drop the old-state check");
        git(&["checkout", "-q", "-b", "side", &format!("{red}^")]);
        let side = land_checks("[land]\n", "a side line");
        let episode = |command: &str| crate::land::red_episode::Episode { head: red.clone(), scope: std::collections::BTreeSet::from(["north-graph".to_owned()]), failing: std::collections::BTreeSet::from([Failure::Step(command.to_owned())]), reds: Default::default() };
        assert!(episode("cargo test -E 'test(/old_state/)'").ended_by("", false, &repo, &head), "on 2026-11-18 main stayed red on the old-state check after .safe-push dropped it");
        assert!(!episode("cargo test -E 'test(/old_state/)'").ended_by("", false, &repo, &side), "a green run on a head that does not descend from the red one ends nothing");
        assert!(!episode("cargo test kept").ended_by("", false, &repo, &head), "a check .safe-push still runs needs a green run of it");
        assert!(!episode("cargo clippy").ended_by("", false, &repo, &head), "a step .safe-push never listed needs a green run of it");
        let mixed = crate::land::red_episode::Episode { failing: std::collections::BTreeSet::from([Failure::Step("cargo test -E 'test(/old_state/)'".to_owned()), Failure::Step("cargo test kept".to_owned())]), ..episode("") };
        let rerun = "safe-push: the Rust checks cover -p north-graph\nsafe-push: land check: cargo test kept (2 s)\n";
        assert!(mixed.ended_by(rerun, false, &repo, &head), "on 2026-11-18 main stayed red when a later red step joined the dropped old-state check, though a green run reran the later one");
        assert!(!mixed.ended_by("safe-push: the Rust checks cover -p north-graph\n", false, &repo, &head), "the step .safe-push still lists needs a green run of it beside a dropped one");
    }
}

#[test]
fn a_red_verdict_arriving_after_a_descendants_green_run_covered_its_checks_opens_no_episode_sends_no_alarm_and_blames_no_lane() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    committed_repo(&repo);
    let commit = |message: &str| {
        let git = |arguments: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
        assert!(git(&["commit", "-q", "--allow-empty", "-m", message]).status.success());
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_owned()
    };
    let mut episode = run();
    episode.log = scratch.path().join("post-land.log");
    episode.outcome = scratch.path().join("post-land.outcome");
    episode.ledger = scratch.path().join("post-land.tsv");
    episode.store = scratch.path().join("ledger.db");
    std::fs::write(&episode.outcome, "failed-step\tcargo nextest run\t/x.log\n").unwrap();
    let sent = std::cell::RefCell::new(Vec::new());
    let at = |head: &str, log: &str, verdict: Verdict| {
        let mut landed = landed(&[head]);
        landed.repo.clone_from(&repo);
        std::fs::write(&episode.log, log).unwrap();
        announce(&landed, &episode, &verdict, |_, message| {
            sent.borrow_mut().push(message.to_owned());
            true
        });
        super::settle::stale_red(&landed, &episode)
    };
    let record = || {
        let store = north_base::lifecycle::Store::open(&episode.store).unwrap();
        crate::land::after_landing_lifecycle::current::<crate::land::after_landing_lifecycle::RedMain>(&store, "owner/repo").map(|record| record.state)
    };
    let red = || Verdict::RedMain("exit 100".to_owned());

    let culprit = commit("drill");
    assert!(!at(&culprit, RED, red()), "the first red of an episode is main's state");
    assert_eq!(record(), Some(crate::land::after_landing_lifecycle::Main::Red));
    let descendant = commit("fix");
    at(&descendant, FULL, Verdict::Green);
    assert_eq!(record(), Some(crate::land::after_landing_lifecycle::Main::Green));
    assert_eq!(sent.borrow().len(), 2, "one red alarm and one green message: {:?}", sent.borrow());

    assert!(at(&culprit, RED, red()), "a late red on the head the descendant's green run covered is stale, so its culprit search is not started");
    assert_eq!(record(), Some(crate::land::after_landing_lifecycle::Main::Green), "the red-main record stays green");
    assert_eq!(sent.borrow().len(), 2, "no alarm is sent: {:?}", sent.borrow());

    assert!(!at(&commit("later"), RED, red()), "a red on a head the green run did not descend from is main's state");
    assert_eq!(record(), Some(crate::land::after_landing_lifecycle::Main::Red));
    assert_eq!(sent.borrow().len(), 3, "{:?}", sent.borrow());
}

#[test]
fn a_red_run_with_no_new_failure_starts_no_search() {
    let failure = |name: &str| Failure::Cargo(name.to_owned());
    let known = std::collections::BTreeSet::from([failure("a::fails"), Failure::Step("cargo nextest run".to_owned())]);
    assert!(super::already_known(&std::collections::BTreeSet::from([failure("a::fails")]), &known), "a red run failing only what the previous verdict failed is searched for nothing");
    assert!(!super::already_known(&std::collections::BTreeSet::from([failure("a::fails"), failure("b::fails")]), &known), "one failure the previous verdict did not have starts a search");
    assert!(!super::already_known(&std::collections::BTreeSet::new(), &known), "a red run whose failures were not read names nothing known");
}

#[test]
fn a_rerun_command_is_composed_for_the_host_that_runs_it() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let head = committed_repo(&repo);
    let mut green = run();
    green.log = scratch.path().join("post-land.log");
    green.outcome = scratch.path().join("post-land.outcome");
    green.ledger = scratch.path().join("post-land.tsv");
    green.store = scratch.path().join("ledger.db");
    let marker = scratch.path().join("post-land-red-owner_repo");
    let failing = std::collections::BTreeSet::from([Failure::Cargo("drill".to_owned()), Failure::Harness("harness/a.test.sh".to_owned())]);
    crate::land::red_episode::Episode { head: "red".to_owned(), scope: Default::default(), failing, reds: Default::default() }.write(&marker).unwrap();
    let mut landed = landed(&[&head]);
    landed.repo = repo;
    let sent = std::cell::RefCell::new(Vec::new());
    std::fs::write(&green.log, "safe-push: the Rust checks cover -p north-landing\n").unwrap();
    let asked = std::cell::RefCell::new(String::new());
    let alarm = |_: &str, message: &str| {
        sent.borrow_mut().push(message.to_owned());
        true
    };
    let chosen = std::cell::Cell::new(0);
    let host = || {
        chosen.set(chosen.get() + 1);
        Some("build-host".to_owned())
    };
    let rerun = |host: &str, command: &str| {
        assert_eq!(host, "build-host", "the rerun runs on the host chosen for it");
        command.clone_into(&mut asked.borrow_mut());
        "test drill ... ok\nland check: harness/a.test.sh passed after 0 s\n".to_owned()
    };
    assert_eq!(super::settle::settle_from(&landed, &green, north_base::platform::wait::after(Duration::ZERO), Verdict::Green.into(), alarm, host, rerun), Verdict::Green);
    assert_eq!(chosen.get(), 1, "the rerun's host is chosen once");
    let episode = crate::land::red_episode::Episode { head: "red".to_owned(), scope: Default::default(), failing: std::collections::BTreeSet::from([Failure::Cargo("drill".to_owned()), Failure::Harness("harness/a.test.sh".to_owned())]), reds: Default::default() };
    assert_eq!(Some(asked.borrow().clone()), episode.rerun_command("build-host"), "the command is composed for the host that runs it");
    assert!(asked.borrow().contains("--exact 'drill'") && asked.borrow().contains("land tests 'harness/a.test.sh'"), "{}", asked.borrow());
    assert!(!marker.exists(), "the red record outlived a green rerun of its tests: {:?}", sent.borrow());
    assert!(sent.borrow().len() == 1 && sent.borrow()[0].contains("main is green again"), "{:?}", sent.borrow());
}

#[test]
fn a_failure_outside_the_base_selection_probes_the_base_before_blaming() {
    let sandbox = "worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state";
    let failing = std::collections::BTreeSet::from([Failure::Cargo(sandbox.to_owned()), Failure::Step("cargo nextest run".to_owned())]);
    let probe = super::Probe::of(&failing);
    assert_eq!(probe, super::Probe::Tests(std::collections::BTreeSet::from([Failure::Cargo(sandbox.to_owned())])), "the probe runs the failed test, not the after-landing selection");
    let red_with_the_test = ["ds-1033", "95afbfbca", "lane-b"];
    let probed = std::sync::Mutex::new(Vec::new());
    let check = |tree: &str| {
        probed.lock().unwrap().push(tree.to_owned());
        let red = match &probe {
            super::Probe::Tests(tests) => tests.contains(&Failure::Cargo(sandbox.to_owned())) && red_with_the_test.contains(&tree),
            _ => tree != "ds-1033",
        };
        let failing = if red { std::collections::BTreeSet::from([sandbox.to_owned()]) } else { std::collections::BTreeSet::new() };
        super::Probed { verdict: if red { Verdict::RedMain(sandbox.to_owned()) } else { Verdict::Green }, failing }
    };
    let mut landed = landed(&["lane-b"]);
    landed.base = "ds-1033".to_owned();
    landed.lanes = vec!["dash".to_owned(), "lane-b".to_owned()];
    landed.tips = vec!["95afbfbca".to_owned(), "lane-b".to_owned()];
    let blamed = super::first_red(landed.tips.len(), |count| super::probed(&landed, count, &check));
    assert_eq!(blamed, None, "no lane of the batch is blamed while its base is red on the failed test");
    assert!(probed.lock().unwrap().iter().any(|tree| tree == "ds-1033"), "the base is probed: {probed:?}");
    let ledger = "1\tfirn\t780be397\tred-main\tbash .github/scripts/land-checks.sh native; log: /l\trevert\t661\n";
    let native = std::collections::BTreeSet::from([Failure::Step("bash .github/scripts/land-checks.sh native".to_owned())]);
    assert!(super::red_before(ledger, "firn", "780be397", &native), "a base whose own run was red on the check blames no lane, as firn's reverts of 2026-11-14 needed");
    assert!(!super::red_before(ledger, "firn", "bee979c2", &native) && !super::red_before(ledger, "north", "780be397", &native));
}

#[test]
fn an_after_landing_suite_is_placed_on_a_build_host_and_never_on_the_control_host_while_one_is_available() {
    use crate::land::slots::graph_selection::{Away, Scope, list};
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("tree");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    let here = scratch.path().join("compiled-here");
    let shell = root.join("bin/land-shell");
    north_foundation::foundation::shim::write_script(&shell, &format!("touch {}\n", here.display()));
    let record = scratch.path().join("record");
    let north = scratch.path().join("north");
    let listing = r#"{"rust-suites":{"north-landing::it":{"binary-id":"north-landing::it","package-name":"north-landing","binary-path":"/x","testcases":{"a::b":{}}}}}"#;
    north_foundation::foundation::shim::write_script(&north, &format!("printf '%s\\n' \"$*\" >> {}\nprintf '%s\\n' 'offload: running in a slot' '{listing}'\n", record.display()));
    let away = Away { program: &north, host: "cpx62b", base: "base", tree: "tree", hold: None, closure: None };
    let listed = list(&root, Some(&away), &Scope::Workspace).unwrap_or_default();
    assert_eq!(listed.keys().collect::<Vec<_>>(), ["north-landing::it"], "the listing the build host printed is the one the selection reads");
    assert!(!here.exists(), "listing an after-landing run's tests compiled the workspace on the control host");
    let asked = std::fs::read_to_string(&record).unwrap();
    assert!(asked.starts_with("capacity offload run --host cpx62b --base base --head tree --timeout"), "the listing was not offered to the build host under its capacity lease: {asked}");
    assert!(asked.contains("CARGO_TARGET_DIR='target' './bin/land-shell' '--dev' '--' 'cargo' 'nextest' 'list'"), "the build host runs the tree's own listing command: {asked}");
    let scoped = list(&root, Some(&away), &Scope::Packages(["north-landing".to_owned()].into())).unwrap_or_default();
    assert_eq!(scoped.keys().collect::<Vec<_>>(), ["north-landing::it"], "a listing of the packages a change reaches takes the same binaries the build host prints");
    let scoped_asked = std::fs::read_to_string(&record).unwrap();
    assert!(scoped_asked.lines().last().is_some_and(|line| line.contains("'-p' 'north-landing'") && !line.contains("--workspace")), "a listing of the reached packages names each with -p, not the whole workspace: {scoped_asked}");
}

#[test]
fn a_red_lane_scoped_check_over_a_batch_range_enters_no_blame() {
    let scratch = tempfile::tempdir().unwrap();
    let outcome = scratch.path().join("post-land.outcome");
    std::fs::write(&outcome, "failed-step\tland scan test-admission kills\t/kills.log\tlane-b\nfailed-step\tcargo clippy\t/clippy.log\nfailed-step\tcargo nextest run\t/nextest.log\t\n").unwrap();
    let failing = std::collections::BTreeSet::from([Failure::Step("land scan test-admission kills".to_owned()), Failure::Step("cargo clippy".to_owned()), Failure::Step("cargo nextest run".to_owned())]);
    let lanes = ["lane-a".to_owned(), "lane-b".to_owned()];
    let (blamed, named) = apart(failing, &outcome, &lanes);
    assert_eq!(blamed, std::collections::BTreeSet::from([Failure::Step("cargo clippy".to_owned()), Failure::Step("cargo nextest run".to_owned())]), "a lane-scoped check naming a lane is not a failure the batch is searched for, one naming none stays");
    assert_eq!(named, [1], "the red names the lane whose own range failed");
}

