use std::cell::RefCell;

use north_base::foundation::pair::{self, Pair, Reading};

use super::{Ran, cases, retried};

const LEASE: &str = r#"{"decision":"RELEASED","lease":"5d0c","start":[["scope",0.03]],"subjectSeconds":8.836}"#;
const LOST: &str = "north capacity offload: the connection ended (ssh exit 1) before the remote host reported the command's exit";
const CASE: &str = "lane_gc_journeys::unlanded_lanes_stay_whatever_their_age_or_dirt_and_are_reported_with_issue_state in north";

#[test]
fn a_rerun_that_exits_100_fails_its_case_and_one_that_loses_its_connection_runs_again() {
    let named = vec![CASE.to_owned()];
    let parent = RefCell::new(vec![Ran::Ended(Some(0), LEASE.to_owned()), Ran::Ended(Some(75), LOST.to_owned()), Ran::Ended(Some(0), LEASE.to_owned())].into_iter());
    let culprit = RefCell::new(vec![Ran::Ended(Some(100), LEASE.to_owned()), Ran::Ended(Some(100), LEASE.to_owned())].into_iter());
    let check = |commit: &str| {
        let runs = if commit == "parent" { &parent } else { &culprit };
        retried(|| runs.borrow_mut().next().unwrap(), cases, &named)
    };
    let compared = pair::compare(&check, "parent", "culprit");
    assert_eq!(compared.candidate[0], Reading::Red(CASE.to_owned()), "on 2026-11-15 the replay of hosted run 43666903477 read fd0d4aa0a7's two exit-100 reruns as no verdict because the slot kept only its lease record");
    assert_eq!(compared.pair, Pair::Regressed, "the parent's rerun lost to the offload connection ran again, so fd0d4aa0a7 trips: {compared:?}");
}
