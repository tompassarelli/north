#![cfg(test)]

use super::recommend_acts::decide;
use crate::hook_input::{HookInput, Verdict};
use serde_json::json;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    format!("{}/src/guard/fixtures/recommend_acts/{name}.jsonl", env!("CARGO_MANIFEST_DIR"))
}

fn stop(name: &str, session: &str) -> HookInput {
    let data = json!({"hook_event_name":"Stop", "agent_type":"coordinator", "session_id":session, "stop_hook_active":false, "transcript_path":fixture(name)});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

fn state(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("north-recommend-acts-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_recommendation_naming_an_ask_once_category_or_only_quoted_passes() {
    assert_eq!(decide(&stop("ask_once", "s1"), &state("ask-once")), Verdict::Allow);
    let turn = super::recommend_acts::Turn::default();
    assert!(super::recommend_acts::recommendation("I recommend minting the GitHub token for the mirror.", &turn).is_none(), "a token is an ask-once decision");
    assert!(super::recommend_acts::recommendation("My recommendation: the second option, as the north gate asks.", &turn).is_none(), "an open north gate is an ask-once decision");
    assert!(super::recommend_acts::recommendation("My recommendation is the second option; answer it with the fleet.", &turn).is_some());
    assert!(super::recommend_acts::recommendation("The refused reply said \"I recommend the second option\" and `want me to` stayed in the code span.", &turn).is_none(), "a phrase only quoted is not a recommendation");
    assert!(super::recommend_acts::recommendation("The refused reply said \"I recommend the first option\". I recommend the second option.", &turn).is_some(), "an unquoted phrase still is");
}
