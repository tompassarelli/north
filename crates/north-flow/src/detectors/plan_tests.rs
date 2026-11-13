use std::collections::BTreeSet;

use super::{Doc, Facts, Landed, stalled};
use super::super::Limits;

const PLAN: &str = "# Plan\n\nOwner: north#77\n\n1. **Build the thing.** Done when it builds.\n";

fn a_plan(text: &str) -> Facts {
    Facts { docs: vec![Doc { slug: "thing".to_owned(), added_at: 0.0, text: text.to_owned() }], ..Facts::default() }
}

fn limits() -> Limits {
    Limits { stall_s: 3600.0, silence_cadences: 1.0, refusal_window_s: 3600.0, refusal_min: 1, disk_floor_gib: 1.0, slow_test_seconds: 1.0, slow_test_runs: 1 }
}

fn keys(facts: &Facts) -> Vec<String> {
    stalled(facts, 7200.0, &limits()).into_iter().map(|finding| finding.key).collect()
}

pub fn a_plan_whose_owning_issue_is_closed_raises_no_stalled_plan_finding() {
    let mut facts = a_plan(PLAN);
    assert_eq!(keys(&facts), ["plan:thing#1"], "an unreadable issue list hides nothing");
    facts.open_issues = Some(BTreeSet::from(["north#77".to_owned()]));
    assert_eq!(keys(&facts), ["plan:thing#1"], "an open owner keeps its finding");
    facts.open_issues = Some(BTreeSet::from(["north#78".to_owned()]));
    assert!(keys(&facts).is_empty(), "a closed owner can no longer staff the box");
    facts.open_issues = Some(BTreeSet::new());
    facts.docs[0].text = PLAN.replace("Owner: north#77\n", "");
    assert_eq!(keys(&facts), ["plan:thing#1"], "a plan without an owner keeps today's behaviour");
    facts.open_issues = None;
    facts.docs[0].text = PLAN.to_owned();
    facts.landed = vec![Landed { at: 10.0, subject: "Build it under another name".to_owned(), boxes: vec!["Build the thing".to_owned()] }];
    assert!(keys(&facts).is_empty(), "a commit whose Box trailer is the box title lands the box");
}
