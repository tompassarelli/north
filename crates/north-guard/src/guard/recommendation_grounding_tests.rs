#![cfg(test)]

use super::recommendation_grounding::{decide, reason, recommendation};
use crate::hook::{HookInput, Verdict, evaluate};
use serde_json::{Value, json};

const NAMED: &str = "south:agents/decisions/harness/gex45-access-plan.md";
const RECOMMENDS: &str = "Landed `abc123`; the runtime builds.\n\nI recommend building the laptop runtime on gex45 and shipping it from there, since gex45 is idle.";

fn recorded(paragraph: &str) -> Vec<String> {
    if paragraph.contains("gex45") { vec![NAMED.to_owned()] } else { Vec::new() }
}

fn stop(message: &str, active: bool) -> HookInput {
    let data = json!({"hook_event_name":"Stop", "stop_hook_active":active, "last_assistant_message":message});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

fn block(output: Option<crate::platform::pyjson::Value>) -> Option<Value> {
    output.map(|value| serde_json::from_str(&crate::platform::pyjson::dumps(&value)).unwrap())
}

#[test]
fn a_recommendation_that_hits_a_decision_record_is_blocked_once_with_the_record_named() {
    let records = recorded;
    let expected = Verdict::Deny(reason(&[NAMED.to_owned()]));
    assert_eq!(decide(&stop(RECOMMENDS, false), "", &records), expected);
    assert_eq!(decide(&stop(RECOMMENDS, false), "proxy", &records), expected);
    assert_eq!(reason(&[NAMED.to_owned()]), format!("These recorded decisions bear on your recommendation: {NAMED}; read them and confirm or revise."));
    assert_eq!(decide(&stop(RECOMMENDS, true), "", &records), Verdict::Allow, "stop_hook_active lets the second stop through");
    assert_eq!(decide(&stop(RECOMMENDS, false), "stop-ask-guard", &records), Verdict::Allow, "a non-proxy fleet worker's report passes");
    let guards: [crate::hook::Guard; 1] = [|input| decide(input, "", &recorded)];
    assert_eq!(block(evaluate(&stop(RECOMMENDS, false), &guards, "Stop")), Some(json!({"decision":"block", "reason":reason(&[NAMED.to_owned()])})));
    assert_eq!(crate::guard::host::recorded_decisions(RECOMMENDS, 0.0), Vec::<String>::new(), "no host installed means no records and no block");
}

#[test]
fn a_recommendation_with_no_hit_and_a_report_with_no_recommendation_pass() {
    let records = recorded;
    for report in [
        "Landed.\n\nI recommend a seeded retry in the flaky landing test.",
        "Landed.\n\nThe fix is a seeded retry in the flaky landing test.",
    ] {
        assert_eq!(decide(&stop(report, false), "", &records), Verdict::Allow, "{report}");
    }
    let untouched = |_: &str| -> Vec<String> { panic!("no recommendation, no retrieval") };
    for report in [
        "Landed `abc123`; the laptop runtime builds on gex45.",
        "Landed.\n\nI recommended the gex45 runtime build earlier.",
        "Landed.\n\nThe fix isn't on gex45.",
    ] {
        assert_eq!(decide(&stop(report, false), "", &untouched), Verdict::Allow, "{report}");
    }
}

#[test]
fn the_recommendation_paragraph_is_the_last_one_carrying_a_marker() {
    for (report, paragraph) in [
        (RECOMMENDS, Some("I recommend building the laptop runtime on gex45 and shipping it from there, since gex45 is idle.")),
        ("Recommended: ship it.\n\nLanded.", Some("Recommended: ship it.")),
        ("Landed.\r\n\r\nThe fix would be a lock.\r\nIt is small.", Some("The fix would be a lock.\nIt is small.")),
        ("I\u{2019}d say I recommend the lock.\n  \nLanded.", Some("I'd say I recommend the lock.")),
        ("the fix is in.", Some("the fix is in.")),
        ("Landed.\n\nNothing to recommend.", None),
    ] {
        assert_eq!(recommendation(report).as_deref(), paragraph, "{report}");
    }
}

#[test]
fn an_ask_and_a_grounded_recommendation_block_together() {
    let report = format!("{RECOMMENDS}\n\nWant me to land it?");
    let guards: [crate::hook::Guard; 2] = [
        |input| crate::guard::permission_offer::decide(input, ""),
        |input| decide(input, "", &|_| vec![NAMED.to_owned()]),
    ];
    let reason = format!("{}\n\n{}", crate::guard::permission_offer::REASON, reason(&[NAMED.to_owned()]));
    assert_eq!(block(evaluate(&stop(&report, false), &guards, "Stop")), Some(json!({"decision":"block", "reason":reason})));
}
