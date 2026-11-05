use proptest::prelude::*;

use super::*;
use crate::usage::decide::{Assessments, assess};

fn sessions() -> impl Strategy<Value = Vec<Session>> {
    let one = (
        prop::sample::select(vec!["a", "b"]),
        prop::sample::select(vec![Owner::Fleet, Owner::Tom]),
        prop::sample::select(vec!["n#1", "n#2"]),
        prop::sample::select(vec!["b1", "b2", "b3"]),
        prop::sample::select(vec![0.0, 1.0, 5.0, 9.0, 40.0]),
        any::<bool>(),
        any::<bool>(),
    );
    prop::collection::vec(one, 0..6).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(|(index, (account, owner, workstream, box_id, spent, lead, parked))| Session {
                owner,
                standing: if lead { Standing::Lead } else { Standing::Worker },
                workstream: Some(Workstream(workstream.to_owned())),
                box_id: Some(BoxId(box_id.to_owned())),
                parked_since: parked.then_some(Unix(NOW.0 - 700)),
                expected: Expected { p50: Usd(2.0), p90: Usd(4.0), minutes: 40.0, source: EstimateSource::Cell("c".to_owned()) },
                ..session(&format!("s{index}"), account, 2.0, Some(spent))
            })
            .collect()
    })
}

fn fixture_with(sessions: Vec<Session>) -> Fixture {
    let mut fixture =
        Fixture::accounts(&["a", "b"]).open("a", 10.0, NOW.0 + 100_000, 100.0, NOW.0 + 9_000).open("b", 10.0, NOW.0 + 100_000, 0.0, NOW.0 + 9_000);
    fixture.sessions = sessions;
    fixture
}

proptest! {
    #[test]
    fn prop_determinism_any_permutation_of_the_sessions_gives_the_same_assessments(sessions in sessions()) {
        let reversed: Vec<Session> = sessions.iter().rev().cloned().collect();
        let (left, right) = (fixture_with(sessions), fixture_with(reversed));
        let first = assess(&left.world(), &Assessments::default());
        prop_assert_eq!(&first, &assess(&right.world(), &Assessments::default()));
        prop_assert_eq!(assess(&left.world(), &first), assess(&right.world(), &first));
    }

    #[test]
    fn prop_tom_exclusion_removing_toms_sessions_never_changes_the_assessments(sessions in sessions()) {
        let without: Vec<Session> = sessions.iter().filter(|session| session.owner != Owner::Tom).cloned().collect();
        let (all, fleet) = (fixture_with(sessions), fixture_with(without));
        prop_assert_eq!(assess(&all.world(), &Assessments::default()), assess(&fleet.world(), &Assessments::default()));
    }

    #[test]
    fn prop_breaker_a_single_tick_spike_never_changes_a_confirmed_spend_assessment(sessions in sessions(), spike in 100.0..500.0f64) {
        let calm = fixture_with(sessions.clone());
        let settled = assess(&calm.world(), &assess(&calm.world(), &Assessments::default()));
        let mut spiked = sessions;
        spiked.iter_mut().for_each(|session| session.spent = Some(Usd(spike)));
        let after = assess(&fixture_with(spiked).world(), &settled);
        for (scope, entry) in &after.spend {
            if let Some(before) = settled.spend.get(scope) {
                let estimated = !matches!(entry.raw, crate::usage::decide::Assessment::Unestimated);
                prop_assert!(!estimated || entry.confirmed == before.confirmed, "{scope:?}");
            }
        }
    }

    #[test]
    fn prop_empty_workstreams_a_workstream_without_a_non_lead_boxed_session_is_unestimated(sessions in sessions()) {
        let fleet_only: Vec<Session> = sessions.into_iter().map(|session| Session { standing: Standing::Lead, ..session }).collect();
        let answer = assess(&fixture_with(fleet_only).world(), &Assessments::default());
        for entry in answer.spend.values() {
            prop_assert_eq!(&entry.confirmed, &crate::usage::decide::Assessment::Unestimated);
        }
    }
}
