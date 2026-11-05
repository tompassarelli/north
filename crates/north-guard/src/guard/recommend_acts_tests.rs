#![cfg(test)]

use super::recommend_acts::{decide, read_turn};
use crate::hook::{HookInput, Verdict};
use serde_json::json;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    format!("{}/src/guard/fixtures/recommend_acts/{name}.jsonl", env!("CARGO_MANIFEST_DIR"))
}

fn stop(name: &str, session: &str) -> HookInput {
    let data = json!({"hook_event_name":"Stop", "agent_type":"proxy", "session_id":session, "stop_hook_active":false, "transcript_path":fixture(name)});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

fn state(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("north-recommend-acts-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn blocked(verdict: &Verdict) -> bool {
    matches!(verdict, Verdict::Deny(reason) if reason.starts_with("BLOCKED (recommend-acts)") && !reason.contains('\n'))
}

#[test]
fn an_unacted_recommendation_blocks_twice_per_turn_and_an_earlier_turns_issue_does_not_count() {
    let dir = state("unacted");
    let turn = read_turn(&fixture("unacted")).unwrap();
    assert_eq!((turn.id.as_str(), turn.commands.as_slice()), ("t2", ["north fleet list --json".to_owned()].as_slice()), "the turn starts at the last real prompt, past the meta Stop feedback");
    let first = decide(&stop("unacted", "s1"), &dir);
    assert!(blocked(&first), "{first:?}");
    assert!(matches!(&first, Verdict::Deny(reason) if reason.contains("\"i recommend\"") && reason.contains("filed no issue")));
    assert!(blocked(&decide(&stop("unacted", "s1"), &dir)));
    assert_eq!(decide(&stop("unacted", "s1"), &dir), Verdict::Allow, "a third stop in the same turn passes");
    assert!(blocked(&decide(&stop("unacted", "s2"), &dir)), "another session keeps its own count");
}

#[test]
fn a_recommendation_filed_staffed_and_cited_passes_and_an_uncited_one_names_the_citation() {
    let dir = state("acted");
    assert_eq!(decide(&stop("acted", "s1"), &dir), Verdict::Allow);
    let mut uncited = stop("acted", "s3");
    uncited.data["last_assistant_message"] = "I recommend moving the lander to gex45; filed and launched.".into();
    assert!(matches!(decide(&uncited, &dir), Verdict::Deny(reason) if reason.contains("cites no issue")));
}

#[test]
fn a_recommendation_naming_an_ask_once_category_passes() {
    assert_eq!(decide(&stop("ask_once", "s1"), &state("ask-once")), Verdict::Allow);
}
