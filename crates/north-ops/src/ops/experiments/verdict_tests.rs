use std::cell::RefCell;

use super::file::{self, Filed, Issues};
use super::registry::{self, Day, Experiment, Issue};
use super::verdict::{Reading, Recommend, Verdict, judge};

fn entry(id: &str) -> Experiment {
    let text = format!(
        "[[experiment]]\nid = \"{id}\"\nchange = \"route tooling onto arm worker\"\nlanded = \"203518cf6\"\nmetric = \"rework_rate\"\ndirection = \"down\"\nbar = 0.05\nmin_n = 15\nbefore = [\"2026-10-21\", \"2026-11-04\"]\nafter = [\"2026-11-04\", \"2026-11-16\"]\nreview = \"2026-11-18\"\nowner = \"tompassarelli/north#636\"\n"
    );
    registry::parse(&text).unwrap().experiments.remove(0)
}

fn reading(value: f64, n: usize) -> Option<Reading> {
    Some(Reading { value: Some(value), n })
}

struct Owner {
    body: RefCell<String>,
}

impl Issues for Owner {
    fn body(&self, _: &Issue) -> Result<(String, bool), String> {
        Ok((self.body.borrow().clone(), true))
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        *self.body.borrow_mut() = body.to_owned();
        Ok(())
    }

    fn reopen(&self, _: &Issue) -> Result<(), String> {
        Ok(())
    }

    fn close(&self, _: &Issue) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn an_inconclusive_policy_loop_experiment_is_extended_or_settled_by_the_loop_without_a_tom_gate() {
    let looped = entry("climb-arm-route-tooling");
    let between = (reading(0.20, 15), reading(0.18, 15));
    assert_eq!(judge(&entry("arm-route-tooling"), between.0, between.1).verdict, Verdict::Decide(Recommend::Keep), "an entry a person registered still goes to Tom");
    assert_eq!(judge(&looped, between.0, between.1).verdict, Verdict::Settle(Recommend::Keep));
    assert_eq!(judge(&looped, reading(0.20, 15), reading(0.21, 15)).verdict, Verdict::Settle(Recommend::Revert));

    let thin = judge(&looped, reading(0.20, 15), reading(0.10, 9));
    let Verdict::Extend { after, review } = thin.verdict.clone() else { panic!("a thin reading extends: {:?}", thin.verdict) };
    assert_eq!((after.start.text(), after.end.text(), review.text()), ("2026-11-04".to_owned(), "2026-11-22".to_owned(), "2026-11-24".to_owned()), "half the 12-day window again");
    assert!(matches!(judge(&looped, None, reading(0.10, 15)).verdict, Verdict::ReMeasure(_)), "a missing reading is a measurement fault, not an inconclusive verdict");

    let owner = Owner { body: RefCell::new("north#636".to_owned()) };
    let state = std::env::temp_dir().join(format!("north-experiments-filed-{}", std::process::id()));
    let mut filed = Filed::load(state.clone());
    for verdict in [&thin, &judge(&looped, between.0, between.1)] {
        let line = file::line(&looped, reading(0.20, 15), reading(0.10, 9), verdict);
        file::file(&owner, &mut filed, &looped, &line, &verdict.verdict).unwrap();
        assert!(!line.contains("Tom"), "{line}");
    }
    assert!(owner.body.borrow().contains("- [x] Extend climb-arm-route-tooling"), "{}", owner.body.borrow());
    let extended = Filed::load(state.clone()).current(&looped);
    assert_eq!((extended.after.end, extended.review), (Day::parse("2026-11-22").unwrap(), Day::parse("2026-11-24").unwrap()), "the extension survives the next run");
    assert!(!Filed::load(state.clone()).has(&extended), "the extended review is judged again");
    std::fs::remove_file(state).unwrap();
}
