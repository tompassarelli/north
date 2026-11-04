use super::*;
use crate::landing::land_test_runner::Case;
use proptest::prelude::*;

const BASE_TREE: &str = "1111111111111111111111111111111111111111";
const LATER_TREE: &str = "2222222222222222222222222222222222222222";
const FILE: &str = "tests/it/landing.rs";

fn at(hours: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::days(20_000) + Duration::hours(hours)
}

fn results(rows: &[(&str, &str, bool)]) -> Results {
    rows.iter().map(|&(test, name, passed)| Case { test: test.into(), name: name.into(), passed }).collect()
}

fn outcome() -> impl Strategy<Value = Option<Seen>> {
    prop_oneof![
        Just(None),
        Just(Some(Seen { passed: true, failed: false })),
        Just(Some(Seen { passed: false, failed: true })),
        Just(Some(Seen { passed: true, failed: true })),
    ]
}

fn universe() -> Vec<(String, String)> {
    ["a.rs", "b.rs"].iter().flat_map(|file| ["x", "y", "z"].iter().map(move |case| (file.to_string(), case.to_string()))).collect()
}

fn build(outcomes: &[Option<Seen>]) -> Results {
    let mut built = Results::default();
    for ((test, case), seen) in universe().iter().zip(outcomes) {
        if let Some(seen) = seen {
            if seen.passed {
                built.see(test, case, true);
            }
            if seen.failed {
                built.see(test, case, false);
            }
        }
    }
    built
}

#[test]
fn judge_names_only_the_lane_red_case_when_the_base_is_red_in_the_same_file() {
    let base = results(&[(FILE, "a", false), (FILE, "b", true), (FILE, "c", true)]);
    let candidate = results(&[(FILE, "a", false), (FILE, "b", false), (FILE, "c", true)]);
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &base, at(0));
    let verdict = judge(&base, &candidate, &quarantine, &[FILE.to_owned()], at(1));
    assert_eq!(verdict, Verdict::Refuse { new: vec![CaseId::new(FILE, "b")] });
}

#[test]
fn judge_lands_a_lane_over_a_base_red_case_it_does_not_change() {
    let base = results(&[(FILE, "a", false), (FILE, "b", true)]);
    let candidate = results(&[(FILE, "a", false), (FILE, "b", true)]);
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &base, at(0));
    assert_eq!(judge(&base, &candidate, &quarantine, &[FILE.to_owned()], at(1)), Verdict::Land);
}

#[test]
fn judge_quarantine_opens_on_a_base_red_case_and_exits_on_a_passing_later_base_tree() {
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &results(&[(FILE, "a", false), (FILE, "b", true)]), at(0));
    let entry = quarantine.get(FILE, "a").unwrap();
    assert_eq!((entry.reason, entry.tree.as_str(), entry.opened_at), (Reason::BaseRed, BASE_TREE, at(0)));
    assert!(quarantine.get(FILE, "b").is_none());
    quarantine.observe_base(BASE_TREE, &results(&[(FILE, "a", true)]), at(1));
    assert!(quarantine.get(FILE, "a").is_some(), "a pass on the tree it was red on is a retry, not an exit");
    quarantine.observe_base(LATER_TREE, &results(&[(FILE, "a", true)]), at(2));
    assert_eq!(quarantine, Quarantine::default());
}

#[test]
fn judge_flaky_base_case_never_exits_on_a_pass_and_exits_when_its_issue_lands() {
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &results(&[(FILE, "a", true), (FILE, "a", false)]), at(0));
    assert_eq!(quarantine.get(FILE, "a").unwrap().reason, Reason::Flaky);
    quarantine.observe_base(LATER_TREE, &results(&[(FILE, "a", true)]), at(1));
    assert_eq!(quarantine.get(FILE, "a").unwrap().reason, Reason::Flaky);
    quarantine.file_issue(&CaseId::new(FILE, "a"), "tompassarelli/north#999");
    quarantine.landed_naming("tompassarelli/north#998");
    assert!(quarantine.get(FILE, "a").is_some());
    quarantine.landed_naming("tompassarelli/north#999");
    assert!(quarantine.get(FILE, "a").is_none());
}

#[test]
fn judge_base_red_case_seen_flaky_later_turns_flaky_and_stays() {
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &results(&[(FILE, "a", false)]), at(0));
    quarantine.observe_base(LATER_TREE, &results(&[(FILE, "a", true), (FILE, "a", false)]), at(1));
    let entry = quarantine.get(FILE, "a").unwrap();
    assert_eq!((entry.reason, entry.opened_at), (Reason::Flaky, at(0)));
}

#[test]
fn judge_flaky_candidate_pass_is_not_green() {
    let base = results(&[(FILE, "a", true)]);
    let candidate = results(&[(FILE, "a", true), (FILE, "a", false)]);
    let verdict = judge(&base, &candidate, &Quarantine::default(), &[FILE.to_owned()], at(0));
    assert_eq!(verdict, Verdict::Refuse { new: vec![CaseId::new(FILE, "a")] });
}

#[test]
fn judge_holds_a_lane_selecting_a_file_quarantined_over_24_hours() {
    let base = results(&[(FILE, "a", false), ("other.rs", "o", true)]);
    let mut quarantine = Quarantine::default();
    quarantine.observe_base(BASE_TREE, &base, at(0));
    let selects = [FILE.to_owned()];
    let elsewhere = ["other.rs".to_owned()];
    assert_eq!(judge(&base, &base, &quarantine, &selects, at(24)), Verdict::Land);
    assert_eq!(judge(&base, &base, &quarantine, &selects, at(25)), Verdict::Hold { files: vec![FILE.to_owned()] });
    assert_eq!(judge(&base, &base, &quarantine, &elsewhere, at(25)), Verdict::Land);
}

proptest! {
    #[test]
    fn judge_new_failures_are_exactly_candidate_reds_outside_base_reds_and_quarantine(
        base in proptest::collection::vec(outcome(), 6),
        candidate in proptest::collection::vec(outcome(), 6),
        quarantined in proptest::collection::vec(any::<bool>(), 6),
    ) {
        let (base, candidate) = (build(&base), build(&candidate));
        let mut quarantine = Quarantine::default();
        quarantine.observe_base(BASE_TREE, &base, at(0));
        for ((test, case), held) in universe().iter().zip(&quarantined) {
            if *held {
                quarantine.observe_base(LATER_TREE, &results(&[(test, case, false)]), at(0));
            }
        }
        let new: BTreeSet<CaseId> = new_failures(&base, &candidate, &quarantine).into_iter().collect();
        for (test, case) in universe() {
            let red_now = candidate.get(&test, &case).is_some_and(|seen| seen.failed);
            let red_before = base.get(&test, &case).is_some_and(|seen| seen.failed);
            let held = quarantine.get(&test, &case).is_some();
            prop_assert_eq!(new.contains(&CaseId::new(&test, &case)), red_now && !red_before && !held);
        }
    }

    #[test]
    fn judge_never_counts_a_flaky_pass_green(
        base in proptest::collection::vec(outcome(), 6),
        later in proptest::collection::vec(outcome(), 6),
    ) {
        let (base, later) = (build(&base), build(&later));
        let mut quarantine = Quarantine::default();
        quarantine.observe_base(BASE_TREE, &base, at(0));
        let before = quarantine.clone();
        quarantine.observe_base(LATER_TREE, &later, at(1));
        for (id, entry) in before.cases() {
            let seen = later.get(&id.test, &id.case);
            if entry.reason == Reason::Flaky || seen.is_some_and(|seen| seen.failed) {
                prop_assert!(quarantine.get(&id.test, &id.case).is_some());
            }
        }
        for (test, case) in later.flaky() {
            prop_assert_eq!(quarantine.get(test, case).map(|entry| entry.reason), Some(Reason::Flaky));
            let verdict = judge(&Results::default(), &later, &Quarantine::default(), &[], at(1));
            let named = matches!(verdict, Verdict::Refuse { ref new } if new.contains(&CaseId::new(test, case)));
            prop_assert!(named);
        }
    }

    #[test]
    fn judge_holds_exactly_when_a_selected_file_has_a_quarantine_past_its_ttl(
        age_minutes in 0i64..3_000,
        selects in any::<bool>(),
    ) {
        let base = results(&[(FILE, "a", false)]);
        let mut quarantine = Quarantine::default();
        quarantine.observe_base(BASE_TREE, &base, at(0));
        let selected = if selects { vec![FILE.to_owned()] } else { vec!["other.rs".to_owned()] };
        let verdict = judge(&base, &base, &quarantine, &selected, at(0) + Duration::minutes(age_minutes));
        let held = selects && Duration::minutes(age_minutes) > QUARANTINE_TTL;
        prop_assert_eq!(verdict, if held { Verdict::Hold { files: vec![FILE.to_owned()] } } else { Verdict::Land });
    }
}
