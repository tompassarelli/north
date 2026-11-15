use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use north_foundation::foundation::pair::{self, Reading};

use super::super::main_runs::{Kept, MainRun, MainRuns};
use super::super::trip::{REVERT_AFTER_SECONDS, Trip, Trips};
use super::{Holds, Screen, now};

const ORIGIN: &str = "0a69d4935d8c3bf3057103ec577546bf6d68b2c5";
const VERIFIED: &str = "4c4d4309e847a5289d588b460a5b3827e756fa70";
const HEAD: &str = "5f1bf417d4476e69f36bb4f3cd22264c243e86a4";
const LATER: &str = "8d7da9642d0000000000000000000000000000aa";
const ANCESTRY: [&str; 4] = [ORIGIN, VERIFIED, HEAD, LATER];
const VERIFIED_RUN: &str = "43639929845";
const HEAD_RUN: &str = "43648011210";
const LATER_RUN: &str = "43648927047";
const TIMING: &str = "cli::fleet::claims::tests::a_claim_from_a_build_host_is_held_when_the_ledger_answers_late";
const ADDED: &str = "runtime_update::follow::a_session_host_moves_onto_each_promoted_runtime_in_place_and_keeps_its_provider_and_sequence";

fn log(cases: &[&str]) -> String {
    cases.iter().map(|case| format!("test\tTest\t2026-11-15T03:55:00.0Z [2026-11-15T03:55:00.000+00:00]         FAIL [   0.526s] north::it {case}\n")).collect()
}

fn screen(dir: &Path, runs: &[(&'static str, &'static str, &'static [&'static str])], fails_again: bool, reverts: &Rc<Cell<u32>>) -> Screen {
    let trips = Trips(dir.join("trips"));
    trips.write_unreproduced(&Trip { head: VERIFIED.to_owned(), run: Some(VERIFIED_RUN.to_owned()), cases: vec![("north".to_owned(), TIMING.to_owned())] }, "green twice at 4c4d4309 and green twice at its parent", now()).unwrap();
    let mut page: Vec<MainRun> = runs.iter().map(|(head, id, _)| MainRun { head: (*head).to_owned(), id: (*id).to_owned(), red: true }).collect();
    page.extend([MainRun { head: VERIFIED.to_owned(), id: VERIFIED_RUN.to_owned(), red: true }, MainRun { head: ORIGIN.to_owned(), id: "43630000000".to_owned(), red: false }]);
    let logs: Vec<(&str, &[&str])> = runs.iter().map(|(_, id, cases)| (*id, *cases)).collect();
    let runs = MainRuns {
        page: Box::new(move |number| Ok((number == 1).then(|| page.clone()))),
        kept: Box::new(move |run| Ok(Kept::Log(log(logs.iter().find(|(id, _)| *id == run).map_or(&[TIMING][..], |(_, cases)| cases))))),
        alert: Box::new(|_| {}),
    };
    let replay = Box::new(move |_: &Trips, asked: &Trip, _: u64| {
        let regressed = fails_again && asked.cases.iter().any(|(_, case)| case == ADDED);
        let read = |commit: &str| if commit == asked.head && regressed { Reading::Red(format!("{ADDED} in north")) } else { Reading::Green("passed".to_owned()) };
        Ok(Some(pair::compare(&read, "parent", asked.head.as_str())))
    });
    let at = |commit: &str| ANCESTRY.iter().position(|named| *named == commit);
    let contains = move |commit: &str, head: &str| Ok(at(commit).zip(at(head)).is_some_and(|(commit, head)| commit <= head));
    let reverts = Rc::clone(reverts);
    let revert = Box::new(move |_: &Trip| {
        reverts.set(reverts.get() + 1);
        Ok(())
    });
    Screen { holds: Holds(dir.join("holds")), contains: Box::new(contains), landed: Box::new(|_| Ok(Vec::new())), answered: Box::new(|_, _| false), after_landing: None, trips, runs, revert, replay }
}

#[test]
fn a_head_red_as_the_last_verified_head_promotes_unless_newly_red() {
    let state = tempfile::tempdir().unwrap();
    let (same, added, flaky) = (state.path().join("same"), state.path().join("added"), state.path().join("flaky"));
    let reverts = Rc::new(Cell::new(0));

    let inherited = screen(&same, &[(HEAD, HEAD_RUN, &[TIMING])], true, &reverts);
    assert!(inherited.order(HEAD, &["whiterabbit"], now()).is_ok(), "on 2026-11-15 the hosted runs of 4c4d4309 and 5f1bf417 both failed the claims timing test, and from 3:55 AM the trip on each later head held it until a hand clear: a head red on the case its last verified head was red on promotes");
    assert!(Trips(same.join("trips")).read(HEAD).unwrap().is_none(), "a red the last verified head already had trips nothing");

    let regressed = screen(&added, &[(LATER, LATER_RUN, &[TIMING, ADDED]), (HEAD, HEAD_RUN, &[TIMING, ADDED])], true, &reverts);
    assert!(regressed.order(LATER, &["whiterabbit"], now()).is_ok(), "a later head goes live and is verified after, so the fix for a trip reaches the hosts it trips");
    assert_eq!(Trips(added.join("trips")).read(HEAD).unwrap().map(|trip| trip.cases), Some(vec![("north".to_owned(), ADDED.to_owned())]), "a run that added a failure is judged though a later run red the same way finished before any screen saw it newest, as the commit review of 67751156b9 found it was not");
    let refused = regressed.order(HEAD, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(refused.contains(ADDED) && !refused.contains(TIMING), "a head whose run adds a failure that fails again on a build host and passes at its parent is refused, naming only that failure: {refused}");
    assert!(regressed.order(LATER, &["whiterabbit"], now() + REVERT_AFTER_SECONDS).is_ok() && reverts.get() == 1, "a trip behind a newer red run still asks its revert, as the commit review of 67751156b9 found it did not");

    let unreproduced = screen(&flaky, &[(HEAD, HEAD_RUN, &[TIMING, ADDED])], false, &reverts);
    assert!(unreproduced.order(HEAD, &["whiterabbit"], now()).is_ok(), "in 58 of 65 red hosted runs from 2026-11-10 to 2026-11-15 the blamed head was not the cause: a failure that does not fail again on a build host trips nothing");
    let trips = Trips(flaky.join("trips"));
    assert!(trips.read(HEAD).unwrap().is_none() && trips.unreproduced(HEAD).unwrap());
    let recorded = fs::read_to_string(flaky.join("trips").join("unreproduced.tsv")).unwrap();
    assert!(recorded.contains(&format!("\t{HEAD}\t{HEAD_RUN}\tnorth\t{ADDED}\t")) && !recorded.contains(&format!("\t{HEAD}\t{HEAD_RUN}\tnorth\t{TIMING}\t")), "the failure that did not fail again is recorded against its test: {recorded}");
}
