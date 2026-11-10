#![cfg(test)]

use super::host::{MessageReads, ReadStreak};
use super::proxy_role::{Budgets, budget_verdict, budgets};
use crate::hook_input::Verdict;
use serde_json::Value;

const TRACE: &str = include_str!("fixtures/proxy_load_trace_2026_11_11.json");

struct Call {
    command: String,
    reads: MessageReads,
}

fn messages() -> Vec<Vec<Call>> {
    let trace: Value = serde_json::from_str(TRACE).expect("the committed trace parses");
    let count = |call: &Value, key: &str| call[key].as_u64().unwrap_or_default() as usize;
    trace["messages"]
        .as_array()
        .expect("the trace holds messages")
        .iter()
        .map(|message| {
            message["calls"]
                .as_array()
                .expect("a message holds calls")
                .iter()
                .map(|call| Call {
                    command: call["command"].as_str().unwrap_or_default().to_owned(),
                    reads: MessageReads { looks: count(call, "looks"), source: count(call, "source"), remote: count(call, "remote"), looking: call["looking"].as_bool().unwrap_or_default() },
                })
                .collect()
        })
        .collect()
}

fn judged(call: &Call, budgets: &Budgets) -> Verdict {
    let streak = ReadStreak { calls: call.reads.looks, subject: "why is the laptop loaded".into(), shapes: vec![(call.command.chars().take(20).collect(), 1)], message: call.reads };
    budget_verdict(&streak, budgets)
}

fn stop_of(message: &[Call], budgets: &Budgets) -> Option<(usize, String)> {
    message.iter().enumerate().find_map(|(at, call)| match judged(call, budgets) {
        Verdict::Deny(reason) => Some((at, reason)),
        _ => None,
    })
}

#[test]
fn the_2026_11_11_load_trace_is_stopped_within_its_look_budget() {
    let budgets = budgets();
    let messages = messages();
    assert_eq!(messages.len(), 2, "two messages from Tom between 4:04 and 4:35 AM +07");
    assert!(messages.iter().map(Vec::len).sum::<usize>() >= 90, "the trace keeps every recorded call");

    for (number, message) in messages.iter().enumerate() {
        let (at, reason) = stop_of(message, &budgets).unwrap_or_else(|| panic!("message {} ran to its end without a stop", number + 1));
        let call = &message[at];
        let within = call.reads.looks <= budgets.looks + 1 && call.reads.source + call.reads.remote <= budgets.reach + 1;
        assert!(within, "message {} was stopped at look {} and reach {}, past the first call over a budget ({} and {})", number + 1, call.reads.looks, call.reads.source + call.reads.remote, budgets.looks, budgets.reach);
        assert!(reason.contains("north work brief new") && reason.contains("north fleet launch"), "message {} was stopped without naming the worker brief: {reason}", number + 1);
        assert!(message[..at].iter().all(|earlier| judged(earlier, &budgets) == Verdict::Allow), "message {} was stopped before its budget", number + 1);
    }
}
