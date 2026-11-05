use super::*;
use crate::landing::land_test_runner::Case;
use proptest::prelude::*;

const BASE_TREE: &str = "1111111111111111111111111111111111111111";
const LATER_TREE: &str = "2222222222222222222222222222222222222222";
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

}
