use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::Duration;

use super::evidence_budget::{Budget, sweep};
use super::tests::{DAY, Machine, touch};
use super::{Inputs, Policy, held, size};

fn run(machine: &Machine, name: &str, age: Duration) -> PathBuf {
    let path = machine.path(&format!("evidence/{name}"));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("frame.png"), vec![7u8; 256 << 10]).unwrap();
    touch(&path.join("frame.png"), machine.now - age);
    touch(&path, machine.now - age);
    path
}

fn swept(machine: &Machine, inputs: &Inputs, runs: u64) -> Vec<PathBuf> {
    let one = size(&machine.path("evidence/oldest"));
    let budget = Budget { bytes: runs * one, min_age: DAY };
    let policy = Policy { idle: DAY, dry_run: false };
    let mut removed = sweep(inputs, &policy, &held::paths(&inputs.proc_root), &budget, 0).unwrap().into_iter().map(|removal| removal.path).collect::<Vec<_>>();
    removed.sort();
    removed
}

fn store(machine: &Machine) -> [PathBuf; 6] {
    [
        run(machine, "oldest", 9 * DAY),
        run(machine, "linked", 8 * DAY),
        run(machine, "held", 7 * DAY),
        run(machine, "older", 6 * DAY),
        run(machine, "newer", 2 * DAY),
        run(machine, "fresh", Duration::from_secs(3600)),
    ]
}

#[test]
fn an_evidence_store_over_budget_loses_its_oldest_runs_but_never_a_fresh_held_or_issue_linked_one() {
    let machine = Machine::new();
    let [oldest, linked, held, older, newer, fresh] = store(&machine);
    machine.dir("proc/4242/fd", DAY);
    symlink(held.join("frame.png"), machine.path("proc/4242/fd/3")).unwrap();
    let inputs = Inputs { open_issue_text: || Ok("[{\"body\":\"frames in ~/.local/state/smashcraft/evidence/linked/frame.png\"}]".into()), ..machine.inputs() };

    assert_eq!(swept(&machine, &inputs, 4), vec![older.clone(), oldest.clone()]);
    assert!(!oldest.exists() && !older.exists() && linked.exists() && held.exists() && newer.exists() && fresh.exists());
}

#[test]
fn an_evidence_store_past_budget_with_only_fresh_runs_left_keeps_them() {
    let machine = Machine::new();
    let [oldest, linked, held, older, newer, fresh] = store(&machine);

    assert_eq!(swept(&machine, &machine.inputs(), 0), vec![held, linked, newer, older, oldest]);
    assert!(fresh.exists());
}

#[test]
fn an_evidence_store_within_budget_keeps_every_run_without_reading_issues() {
    let machine = Machine::new();
    let runs = store(&machine);
    let inputs = Inputs { open_issue_text: || panic!("read open issues while within budget"), ..machine.inputs() };

    assert!(swept(&machine, &inputs, 6).is_empty());
    assert!(runs.iter().all(|run| run.exists()));
}

#[test]
fn an_evidence_store_over_budget_keeps_every_run_when_open_issues_are_unreadable() {
    let machine = Machine::new();
    let runs = store(&machine);
    let inputs = Inputs { open_issue_text: || Err("gh: HTTP 502".into()), ..machine.inputs() };

    assert!(swept(&machine, &inputs, 1).is_empty());
    assert!(runs.iter().all(|run| run.exists()));
}

#[test]
fn an_issue_mention_that_ends_a_sentence_still_keeps_its_run() {
    let machine = Machine::new();
    let [_, linked, ..] = store(&machine);
    let inputs = Inputs { open_issue_text: || Ok("[{\"body\":\"Results are in evidence/oldest.\"}]".into()), ..machine.inputs() };

    assert_eq!(swept(&machine, &inputs, 5), vec![linked]);
}
