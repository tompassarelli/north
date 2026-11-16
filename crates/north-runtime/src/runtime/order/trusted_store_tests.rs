use north_foundation::test_inputs::git::git_in as git;
use north_runtime_records::after_landing::Landing;
use super::super::main_runs::{FIXTURE_ANCESTOR, MainRuns};
use super::super::review_hold::{Screen, contains_in};
use north_worker_sandbox::hold_ledger::{Ask, Caller, Holds};
use super::super::trip::Trips;
use super::super::update::{main_commit, trusted_store_of};
use super::resolve;
use std::fs;
use std::path::Path;

fn commit(checkout: &Path, text: &str) -> String {
    fs::write(checkout.join("f"), text).unwrap();
    git(checkout, &["add", "f"]);
    git(checkout, &["commit", "-qm", text.trim()]);
    git(checkout, &["rev-parse", "HEAD"])
}

pub(super) fn a_promotion_an_order_and_a_review_screen_refuse_what_a_worker_writes_into_the_main_store() {
    let dir = tempfile::tempdir().unwrap();
    let (origin, main, rogue) = (dir.path().join("origin.git"), dir.path().join("main"), dir.path().join("rogue.git"));
    for bare in [&origin, &rogue] {
        git(dir.path(), &["init", "-q", "--bare", "-b", "main", &bare.to_string_lossy()]);
    }
    git(dir.path(), &["clone", "-q", &origin.to_string_lossy(), &main.to_string_lossy()]);
    let held = commit(&main, "held\n");
    let head = commit(&main, "head\n");
    git(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let store = trusted_store_of(&main).unwrap();
    git(&main, &["checkout", "-q", "--orphan", "forged"]);
    let forged = commit(&main, "forged\n");
    git(&main, &["push", "-q", &rogue.to_string_lossy(), "HEAD:refs/heads/main"]);
    git(&main, &["remote", "set-url", "origin", &rogue.to_string_lossy()]);
    git(&main, &["update-ref", "refs/remotes/origin/main", &forged]);
    git(&main, &["replace", &head, &forged]);
    assert_eq!(main_commit(&store).unwrap(), head, "a promotion takes origin's head, not the origin/main a worker moved in the main checkout's store (south#138)");
    assert!(resolve(&store, Some(&forged)).is_err(), "an order refuses a commit origin never sent, though the main checkout's origin/main names it (south#138)");
    let screen = |name: &str, commit: &str| {
        let holds = Holds(dir.path().join(name));
        holds.serve(Ask::Place { commit: commit.to_owned(), lane: name.to_owned(), host: None, until: None, why: "under review".to_owned() }, &Caller { session: "reviewer".to_owned(), coordinator: false, reviewer: true }, 1).unwrap();
        let contains = contains_in(store.clone());
        Screen { holds, contains: Box::new(move |commit, head| Ok(commit == FIXTURE_ANCESTOR || contains(commit, head)?)), landed: Box::new(|_| Ok(Vec::new())), answered: Box::new(|_, _| false), after_landing: Some(Box::new(|_| Landing::Green)), trips: Trips(dir.path().join("trips")), runs: MainRuns::none(), revert: Box::new(|_| Ok(())), replay: crate::runtime::replay::regressing() }
    };
    let refused = screen("held", &held).order(&head, &[], 2).expect_err("a replace ref in the main checkout's store does not hide the held commit from the screen (south#138)");
    assert!(refused.contains(&held), "{refused}");
    assert!(screen("unlanded", &forged).order(&head, &[], 2).is_ok(), "a hold on a lane commit the mirror lacks refuses no head the mirror holds");
}
