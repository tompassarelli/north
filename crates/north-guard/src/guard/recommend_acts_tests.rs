#![cfg(test)]

use super::recommend_acts::{Issue, decide, read_turn, turn_in};
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
    assert!(matches!(&first, Verdict::Deny(reason) if reason.contains("\"i recommend\"") && reason.contains("created or edited no issue")));
    assert!(blocked(&decide(&stop("unacted", "s1"), &dir)));
    assert_eq!(decide(&stop("unacted", "s1"), &dir), Verdict::Allow, "a third stop in the same turn passes");
    assert!(blocked(&decide(&stop("unacted", "s2"), &dir)), "another session keeps its own count");
}

#[test]
fn a_recommendation_filed_with_the_url_gh_printed_and_cited_passes_and_an_uncited_one_names_the_citation() {
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

#[test]
fn a_recommendation_that_cites_an_issue_the_turn_only_staffed_is_blocked_naming_the_missing_box_and_passes_once_the_turn_edited_it() {
    let dir = state("cited-box");
    let launched = decide(&stop("launched_only", "s1"), &dir);
    assert!(blocked(&launched), "{launched:?}");
    assert!(matches!(&launched, Verdict::Deny(reason) if reason.contains("no box tracks this on north#633") && reason.contains("gh issue edit N")));
    assert_eq!(decide(&stop("edited", "s2"), &dir), Verdict::Allow);
}

#[test]
fn an_edited_issue_does_not_cover_a_citation_of_another_issue() {
    let other = decide(&stop("other_issue", "s1"), &state("other-issue"));
    assert!(blocked(&other), "{other:?}");
    assert!(matches!(&other, Verdict::Deny(reason) if reason.contains("cites north#640") && reason.contains("only created or edited north#633")));
}

#[test]
fn the_edited_issue_is_the_first_positional_and_its_repo_may_come_before_the_issue_verb() {
    let moved = |command: &str| {
        let lines = format!("{}\n{}\n", r#"{"type":"user","uuid":"t","message":{"role":"user","content":"x"}}"#, json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Bash","input":{"command":command}}]}}));
        turn_in(lines.as_bytes()).unwrap().moved
    };
    let issue = |repo: Option<&str>, number| Issue { repo: repo.map(str::to_owned), number };
    assert_eq!(moved("gh issue edit -R tompassarelli/north --title 'Split 3 checks' 633"), vec![]);
    assert_eq!(moved("gh issue edit 633 -R tompassarelli/north --add-label x"), vec![issue(Some("north"), 633)]);
    assert_eq!(moved("gh -R tompassarelli/south issue edit 633"), vec![issue(Some("south"), 633)]);
}

#[test]
fn a_recommendation_about_a_token_or_an_open_north_gate_is_an_ask_once_decision() {
    let turn = super::recommend_acts::Turn::default();
    assert!(super::recommend_acts::recommendation("I recommend minting the GitHub token for the mirror.", &turn).is_none());
    assert!(super::recommend_acts::recommendation("My recommendation is the second option; answer it with the fleet.", &turn).is_some());
    assert!(super::recommend_acts::recommendation("My recommendation: the second option, as the north gate asks.", &turn).is_none());
}
