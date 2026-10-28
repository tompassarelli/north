use super::decide;
use super::issues::{Issue, IssueRef};
use super::plan::Cloud;
use super::tests::{fleet, issue, survey};

#[test]
fn an_open_issue_titled_with_a_frozen_topic_is_skipped_with_the_reason_and_others_still_launch() {
    let (hosts, accounts) = fleet();
    let titled = |number: u64, title: &str| Issue { title: title.into(), ..issue("smashcraft", number, "- [ ] a", &[]) };
    let mut frozen = survey(
        vec![titled(1, "Kobold rope hitbox"), titled(2, "Fix the FORSAKEN PALADIN slam"), titled(3, "Anub'arak scorecard"), Issue { body: "- [ ] a\nCategory: feature\nkobold is mentioned in passing".into(), ..titled(4, "Roadmap ledger") }],
        Cloud::Unreachable("the credit login is on nexus".into()),
        hosts,
        accounts,
    );
    frozen.frozen = vec![crate::platform::frozen_topics::Topic {
        repo: "smashcraft".into(),
        name: "Kobold".into(),
        terms: vec!["kobold".into(), "forsaken paladin".into()],
        since: "2026-10-26".into(),
        reason: "being replaced".into(),
    }];
    let outcome = decide(&frozen, "test");
    let skipped = |number: u64| outcome.skipped.iter().find(|(item, _)| *item == IssueRef::new("smashcraft", number)).map(|(_, why)| why.as_str());
    assert!(skipped(1).is_some_and(|why| why.contains("frozen") && why.contains("until Tom permits")), "{:?}", outcome.skipped);
    assert!(skipped(2).is_some_and(|why| why.contains("frozen")), "{:?}", outcome.skipped);
    assert_eq!((skipped(3), skipped(4)), (None, None), "only titles match, never bodies");
    let launched: Vec<u64> = outcome.launches.iter().map(|launch| launch.item.number).collect();
    assert!(!launched.is_empty() && launched.iter().all(|number| *number > 2), "{launched:?}");
}
