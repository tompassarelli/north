#![cfg(test)]

use super::host::{MessageReads, ReadStreak};
use super::proxy_role::{Budgets, ClaudeStart, Registered, World, budget_verdict, budgets, decide};
use crate::hook_input::{HookInput, Verdict};
use serde_json::{Value, json};

const LOAD_TRACE: &str = include_str!("fixtures/proxy_load_trace_2026_11_11.json");
const TRACED_LAUNCH: &str = include_str!("fixtures/proxy_traced_launch_2026_11_11.json");

struct Proxy;

impl World for Proxy {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/home/tom".into()),
            "NORTH_FLEET_ROLE" => Some("proxy".into()),
            _ => None,
        }
    }
    fn registered_proxy(&self) -> Option<Registered> {
        None
    }
    fn profile(&self, _: &str) -> Option<String> {
        None
    }
    fn claude_start(&self) -> Option<ClaudeStart> {
        None
    }
    fn lane_files(&self, _: &str) -> Option<Vec<String>> {
        None
    }
    fn break_glass(&self) -> bool {
        false
    }
    fn status_verb(&self, words: &[&str]) -> bool {
        matches!(words, ["land", "status", ..] | ["glance", ..])
    }
}

struct Call {
    tool: String,
    input: Value,
    cwd: String,
    reads: MessageReads,
}

fn messages(trace: &str) -> Vec<Vec<Call>> {
    let trace: Value = serde_json::from_str(trace).expect("the committed trace parses");
    let count = |call: &Value, key: &str| call[key].as_u64().unwrap_or_default() as usize;
    let text = |call: &Value, key: &str, or: &str| call[key].as_str().unwrap_or(or).to_owned();
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
                    tool: text(call, "tool", "Bash"),
                    input: call.get("input").cloned().unwrap_or_else(|| json!({"command": text(call, "command", "")})),
                    cwd: text(call, "cwd", "/home/tom/code/north/main"),
                    reads: MessageReads { looks: count(call, "looks"), looking: call["looking"].as_bool().unwrap_or_default() },
                })
                .collect()
        })
        .collect()
}

fn refused_before(call: &Call) -> Verdict {
    let data = json!({"hook_event_name": "PreToolUse", "session_id": "130b1ab7", "cwd": call.cwd, "tool_name": call.tool, "tool_input": call.input});
    decide(&Proxy, &HookInput::parse(data.to_string().as_bytes()).expect("a hook payload"))
}

fn stopped_after(call: &Call, budgets: &Budgets) -> Verdict {
    let streak = ReadStreak { calls: call.reads.looks, subject: "a question from Tom".into(), shapes: vec![(call.tool.clone(), 1)], message: call.reads };
    budget_verdict(&streak, budgets)
}

fn stop_of(message: &[Call], budgets: &Budgets) -> Option<(usize, String)> {
    message.iter().enumerate().find_map(|(at, call)| match (refused_before(call), stopped_after(call, budgets)) {
        (Verdict::Deny(reason), _) | (_, Verdict::Deny(reason)) => Some((at, reason)),
        _ => None,
    })
}

#[test]
fn the_2026_11_11_load_trace_is_refused_within_its_look_budget() {
    let budgets = budgets();
    let messages = messages(LOAD_TRACE);
    assert_eq!(messages.len(), 2, "two messages from Tom between 4:04 and 4:35 AM +07");
    assert!(messages.iter().map(Vec::len).sum::<usize>() >= 90, "the trace keeps every recorded call");

    for (number, message) in messages.iter().enumerate() {
        let (at, reason) = stop_of(message, &budgets).unwrap_or_else(|| panic!("message {} ran to its end without a refusal", number + 1));
        assert!(message[at].reads.looks <= budgets.looks + 1, "message {} was first refused at look {}, past the first call over the budget of {}", number + 1, message[at].reads.looks, budgets.looks);
        assert!(reason.contains("north work brief new") && reason.contains("north fleet launch") && !reason.contains("STOPPED"), "message {} was refused without naming the worker brief, or ended the turn: {reason}", number + 1);
    }
}

#[test]
fn the_2026_11_11_traced_launch_on_nexus_is_refused() {
    let budgets = budgets();
    let messages = messages(TRACED_LAUNCH);
    let calls: Vec<&Call> = messages.iter().flatten().collect();
    let traced = calls.iter().position(|call| call.input["command"].as_str().is_some_and(|command| command.contains("ssh") && command.contains("strace"))).expect("the trace holds the traced launch on nexus");

    let (at, reason) = stop_of(&messages[0], &budgets).expect("the traced launch's message ran to its end without a stop");
    assert!(at <= traced, "the guard let the traced launch run, stopping the message at call {at}");
    assert!(reason.contains("proxy-reach-guard") && reason.contains("`Item:`") && reason.contains("north fleet launch"), "{reason}");
    let Verdict::Deny(reason) = refused_before(calls[traced]) else { panic!("the traced launch on nexus ran") };
    assert!(reason.contains("strace") && reason.contains("nexus"), "{reason}");
}
