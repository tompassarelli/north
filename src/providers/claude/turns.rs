use serde_json::Value;

use crate::fleet::runtime::CredentialFailure;
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};

pub(super) fn signals(frame: &Value) -> Vec<TurnSignal> {
    let Some(conversation) = frame["session_id"].as_str().map(str::to_owned) else { return Vec::new() };
    match frame["type"].as_str() {
        Some("system") if frame["subtype"] == "init" => {
            vec![TurnSignal::Opened { conversation, model: frame["model"].as_str().map(str::to_owned), effort: None }]
        }
        Some("user") if has_user_message(frame) => {
            let started = TurnSignal::Started { conversation: conversation.clone() };
            if frame["origin"]["kind"] == "task-notification" {
                vec![started]
            } else {
                vec![started, TurnSignal::Prompt { conversation, text: replay_text(frame) }]
            }
        }
        Some("assistant") if frame["parent_tool_use_id"].is_null() => {
            let call = call_signal(frame, &conversation);
            [Some(TurnSignal::Started { conversation }), call].into_iter().flatten().collect()
        }
        Some("system") if frame["subtype"] == "task_started" => task_signal(frame, |task| TurnSignal::TaskStarted { conversation, task }),
        Some("system") if frame["subtype"] == "task_notification" => task_signal(frame, |task| TurnSignal::TaskEnded { conversation, task }),
        Some("result") => vec![TurnSignal::Ended { conversation, exit: result_exit(frame), totals: result_totals(frame) }],
        _ => Vec::new(),
    }
}

fn call_signal(frame: &Value, conversation: &str) -> Option<TurnSignal> {
    let message = &frame["message"];
    let usage = &message["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let tokens = Tokens { input: count("input_tokens"), output: count("output_tokens"), cache_read: count("cache_read_input_tokens"), cache_write: count("cache_creation_input_tokens") };
    Some(TurnSignal::Call { conversation: conversation.to_owned(), call: message["id"].as_str()?.to_owned(), tokens }).filter(|_| usage.is_object())
}

pub(super) fn credential_failure(frame: &Value) -> Option<CredentialFailure> {
    let text = match frame["type"].as_str() {
        Some("result") => frame["result"].as_str(),
        Some("assistant") => frame["message"]["content"][0]["text"].as_str(),
        _ => None,
    };
    match text? {
        text if text.starts_with("Not logged in") => Some(CredentialFailure::SignedOut),
        text if text.contains("OAuth session expired") => Some(CredentialFailure::Expired),
        _ => None,
    }
}

fn has_user_message(frame: &Value) -> bool {
    match &frame["message"]["content"] {
        Value::String(_) => true,
        Value::Array(blocks) => blocks.iter().any(|block| block["type"] != "tool_result"),
        _ => false,
    }
}

fn task_signal(frame: &Value, signal: impl FnOnce(String) -> TurnSignal) -> Vec<TurnSignal> {
    frame["task_id"].as_str().map(|task| signal(task.to_owned())).into_iter().collect()
}

fn replay_text(frame: &Value) -> String {
    let content = &frame["message"]["content"];
    match content.as_str() {
        Some(text) => text.to_owned(),
        None => content.as_array().into_iter().flatten().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"),
    }
}

fn result_exit(frame: &Value) -> TurnExit {
    let aborted = frame["terminal_reason"].as_str().is_some_and(|reason| reason.starts_with("aborted"));
    match (aborted, frame["is_error"] == true) {
        (true, _) => TurnExit::Interrupted,
        (false, true) => TurnExit::Failed,
        (false, false) => TurnExit::Completed,
    }
}

fn result_totals(frame: &Value) -> TurnTotals {
    let usage = &frame["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    TurnTotals {
        tokens: usage.is_object().then(|| Tokens {
            input: count("input_tokens"),
            output: count("output_tokens"),
            cache_read: count("cache_read_input_tokens"),
            cache_write: count("cache_creation_input_tokens"),
        }),
        turns: frame["num_turns"].as_u64(),
        wall_ms: frame["duration_ms"].as_u64(),
    }
}

