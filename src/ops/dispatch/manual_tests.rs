use super::grounds::Grounds;
use super::manual::{Ask, Start, start};
use super::record::Record;
use super::tier::{Evidence, History};
use std::collections::{BTreeMap, BTreeSet};

const CFG: &str = r#"
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
[tiers."claude:sonnet-high"]
model = "claude-sonnet-5-5"
effort = "high"
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
"#;

fn launch(brief: &str, model: Option<&str>, reason: Option<&str>, history: &History) -> Result<Start, String> {
    let ask = Ask { item: "north#9", brief, prompt: brief, model, reason };
    start(&ask, &CFG.parse().unwrap(), history, &BTreeMap::<String, Evidence>::new(), &|_| Grounds::default())
}

fn run(category: &str) -> Start {
    launch(&format!("Item: north#9 x\nCategory: {category}\nSpec: exact\n\nDo it."), None, None, &History::default()).unwrap()
}

fn tried(tier: &str, category: &str, arm: bool) -> Record {
    Record {
        at: 1.0,
        item: "north#9".into(),
        host: "nexus".into(),
        account: "claude:c".into(),
        name: "n9-first".into(),
        cloud: false,
        launched: true,
        detail: String::new(),
        tier: Some(tier.into()),
        category: Some(category.into()),
        arm,
        handoff: None,
        hand: true,
    }
}

#[test]
fn a_known_cause_brief_starts_on_sonnet_high_and_a_mechanical_one_on_haiku_with_its_wait_rule() {
    let known = run("bug-known-cause");
    assert_eq!((known.model.as_str(), known.effort.as_deref(), known.route.tier.name()), ("claude-sonnet-5-5", Some("high"), "sonnet-high"));
    assert!(known.prompt.starts_with("Item: north#9 x\nCategory: bug-known-cause\nSpec: exact\nTier: sonnet-high\nArm: sonnet-high\n\nDo it."), "{}", known.prompt);
    let mechanical = run("mechanical");
    assert_eq!((mechanical.model.as_str(), mechanical.route.tier.name()), ("claude-haiku-5-5", "worker-haiku"));
    assert!(mechanical.prompt.contains("Tier: worker-haiku") && mechanical.prompt.ends_with("never end your turn while a run you started is still going."), "{}", mechanical.prompt);
    let unknown = run("debugging-unknown-cause");
    assert_eq!((unknown.model.as_str(), unknown.effort.as_deref()), ("claude-opus-5-5", Some("medium")));
}

#[test]
fn a_model_that_differs_from_the_pick_is_refused_without_a_tier_reason_and_runs_with_one() {
    let brief = "Item: north#9 x\nCategory: bug-known-cause";
    let refused = launch(brief, Some("opus"), None, &History::default()).unwrap_err();
    assert!(refused.contains("--tier-reason") && refused.contains("sonnet-high"), "{refused}");
    let allowed = launch(brief, Some("opus"), Some("the cause spans two engines"), &History::default()).unwrap();
    assert_eq!((allowed.model.as_str(), allowed.effort, allowed.overridden.as_deref()), ("opus", None, Some("the cause spans two engines")));
    assert_eq!(allowed.prompt, brief, "an override stamps no tier or arm for the ledger");
    let same = launch(brief, Some("sonnet"), None, &History::default()).unwrap();
    assert_eq!((same.model.as_str(), same.effort.as_deref(), same.overridden), ("sonnet", Some("high"), None), "an alias of the pick's own model needs no reason");
    let spare = launch(brief, Some("sonnet"), Some("unneeded"), &History::default()).unwrap();
    assert_eq!(spare.overridden, None, "a reason for the pick itself is no override");
}

#[test]
fn tooling_and_features_take_the_arm_after_the_recorded_one_and_stamp_it() {
    let first = run("tooling");
    assert_eq!((first.route.tier.name(), first.route.arm), ("sonnet-high", true));
    assert!(first.prompt.contains("Tier: sonnet-high\nArm: sonnet-high\n"), "{}", first.prompt);
    let history = History::new(&[tried("sonnet-high", "tooling", true)]);
    let next = launch("Item: north#9 x\nCategory: tooling", None, None, &history).unwrap();
    assert_eq!((next.route.tier.name(), next.model.as_str()), ("worker", "claude-opus-5-5"));
    assert!(next.prompt.contains("Arm: worker"), "{}", next.prompt);
}

#[test]
fn a_haiku_miss_retries_on_sonnet_and_names_it_in_follows() {
    let history = History::new(&[tried("worker-haiku", "mechanical", false)]);
    let retry = launch("Item: north#9 x\nCategory: mechanical", None, None, &history).unwrap();
    assert_eq!(retry.route.tier.name(), "sonnet-high");
    assert!(retry.prompt.contains("Follows: n9-first (worker-haiku)"), "{}", retry.prompt);
}

#[test]
fn a_brief_without_a_category_is_a_feature_and_a_prompt_without_a_header_gets_one() {
    let bare = launch("Item: north#9 x", None, None, &History::default()).unwrap();
    assert_eq!(bare.route.category, "feature");
    assert!(bare.prompt.contains("Category: feature\nTier: sonnet-high\nArm: sonnet-high"), "{}", bare.prompt);
    let ask = Ask { item: "north#9", brief: "Category: bug-known-cause", prompt: "Read /tmp/brief.md and start.", model: None, reason: None };
    let headed = start(&ask, &CFG.parse().unwrap(), &History::default(), &BTreeMap::new(), &|_| Grounds::default()).unwrap();
    assert_eq!(headed.prompt, "Tier: sonnet-high\nArm: sonnet-high\n\nRead /tmp/brief.md and start.");
}

#[test]
fn a_dispatched_brief_that_names_its_tier_keeps_it_and_is_not_stamped_or_recorded_again() {
    let brief = "Item: smashcraft#3 x\nCategory: tooling\nTier: worker\nArm: worker";
    let dispatched = launch(brief, Some("claude-opus-5-5"), None, &History::default()).unwrap();
    assert_eq!((dispatched.routed, dispatched.prompt.as_str(), dispatched.overridden), (true, brief, None));
    assert!(launch(brief, Some("claude-sonnet-5-5"), None, &History::default()).unwrap_err().contains("worker"), "the named tier is the pick");
}

#[test]
fn a_brief_naming_an_escalation_tier_keeps_it_and_its_effort_and_an_unknown_tier_is_refused() {
    let high = launch("Item: north#9 x\nCategory: bug-known-cause\nTier: worker-high", None, None, &History::default()).unwrap();
    assert_eq!((high.model.as_str(), high.effort.as_deref(), high.routed), ("claude-opus-5-5", Some("high"), true));
    assert_eq!(high.prompt, "Item: north#9 x\nCategory: bug-known-cause\nTier: worker-high", "no second Tier line");
    let xhigh = launch("Item: north#9 x\nTier: worker-xhigh", Some("opus"), None, &History::default()).unwrap();
    assert_eq!((xhigh.model.as_str(), xhigh.effort.as_deref()), ("opus", Some("xhigh")));
    let sonnet = launch("Item: north#9 x\nTier: worker-sonnet", None, None, &History::default()).unwrap();
    assert_eq!((sonnet.model.as_str(), sonnet.routed), ("claude-sonnet-5-5", true));
    let unknown = launch("Item: north#9 x\nTier: fable", None, None, &History::default()).unwrap_err();
    assert!(unknown.contains("names no tier"), "{unknown}");
}

#[test]
fn a_hand_launch_holds_nothing_and_contests_nothing_so_a_restaff_within_two_hours_is_not_refused() {
    let hand = tried("worker-haiku", "mechanical", false);
    let twice = [hand.clone(), Record { at: 2.0, ..hand }];
    assert!(super::record::holds(&twice, 100.0, &BTreeMap::new(), &BTreeSet::new()).is_empty());
    assert!(super::record::launching(&twice, 100.0, &BTreeSet::new()).is_empty());
    assert!(super::record::contested(&twice, 100.0).is_empty());
}
