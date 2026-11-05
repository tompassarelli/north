#![cfg(test)]

use super::recommendation_grounding::{decide, reason};
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

