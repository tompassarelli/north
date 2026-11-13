#![cfg(test)]

use super::brief_skeleton::skeleton;
use super::worker_tier::{brief_refusal, routing_refusal};
use crate::router::tier::Tier;

const HEADER: &str = "Category: review\nSpec: exact\nScope: module\nSurface: rust\nVerify: local-test\n";

fn routing(kind: &str, prompt: &str) -> Option<String> {
    routing_refusal(kind, false, prompt, &|_| Some(Tier::Opus))
}

#[test]
fn a_brief_that_names_its_issue_or_gives_its_model_reason_and_cites_a_rule_by_number_is_judged_by_its_header() {
    let named = format!("{HEADER}\nReview north#621 box P12a, read-only.");
    assert_eq!(routing("worker", &named), None, "an issue named in the brief is its item");
    let cited = format!("{HEADER}\nRepo policy (north#635): every added test must survive.");
    assert!(routing("worker", &cited).is_some_and(|why| why.starts_with("Missing Item:")), "a rule cited by number is not the item");
    let item = format!("Item: north#621\n{HEADER}");
    for reason in ["Tier reason: a fresh reviewer of another tier.", "Sonnet because the review needs a tier different from the author.", "Model reason: Sonnet, a fixed rubric."] {
        assert_eq!(routing("worker-sonnet", &format!("{item}\n{reason}")), None, "{reason}");
    }
    assert!(routing("worker-sonnet", &item).is_some_and(|why| why.contains("routing picks")), "no reason, no exemption");

    let later_line = format!("{item}\nTask: reply with a line `Verify: the verdict` for each test.");
    assert_eq!(brief_refusal("worker", false, false, &later_line), None, "only the header's facet line is judged");
    let bad = item.replace("Verify: local-test", "Verify: reading");
    assert!(brief_refusal("worker", false, false, &bad).is_some_and(|why| why.contains("not a facet value")));

    let over = skeleton("tooling", &[], true).expect("tooling is a category");
    assert_eq!(over.lines().filter(|line| line.contains("no net-new test")).count(), 1, "{over}");
    assert!(!skeleton("tooling", &[], false).expect("tooling is a category").contains("net-new test"));
}

#[test]
fn a_native_brief_that_lands_nothing_needs_no_lands_before_line() {
    let native = HEADER.replace("local-test", "native");
    let landing = format!("Item: north#1\n{}", native.replace("review", "feature"));
    assert!(brief_refusal("worker", false, false, &landing).is_some_and(|why| why.contains("Lands-before")));
    assert_eq!(brief_refusal("worker", false, false, &format!("Item: north#1\n{native}\nRead the four images.")), None, "a review lands nothing");
    let investigate = format!("Item: north#1\n{}\nInvestigate only; no landing needed.", landing.replace("Item: north#1\n", ""));
    assert_eq!(brief_refusal("worker", false, false, &investigate), None);
}
