use super::*;
use serde_json::json;

struct Fake {
    claimed: Vec<&'static str>,
    open: Vec<(u64, &'static str)>,
}

impl World for Fake {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        Some(named.unwrap_or("tompassarelli/north").to_owned())
    }

    fn claimed(&mut self) -> Vec<String> {
        self.claimed.iter().map(|item| (*item).to_owned()).collect()
    }

    fn open(&mut self, _: &str) -> Vec<(u64, String)> {
        self.open.iter().map(|(number, title)| (*number, (*title).to_owned())).collect()
    }
}

fn world() -> Fake {
    Fake {
        claimed: vec!["north#316"],
        open: vec![
            (300, "Make the open-issue count measure open outcomes in the filing guard"),
            (301, "Harness failing units on whiterabbit"),
        ],
    }
}

fn pre(command: &str) -> HookInput {
    let payload = json!({"tool_name": "Bash", "hook_event_name": "PreToolUse", "tool_input": {"command": command}});
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

fn decide(command: &str) -> Verdict {
    decide_with(&pre(command), &mut world())
}

fn denial(verdict: Verdict) -> String {
    match verdict {
        Verdict::Deny(reason) => reason,
        other => panic!("expected deny, got {other:?}"),
    }
}

#[test]
fn a_blocker_naming_what_it_blocks_passes_even_beside_the_agents_own_claim() {
    for command in [
        "gh issue create --title 'Cargo registry mirror is unreachable' --body $'Blocks: tompassarelli/north#316\\n\\nBody'",
        "gh issue create -t 'Cargo registry mirror is unreachable' --body-file - <<'EOF'\nBlocks: north#316\nSplit of north#316\nEOF",
    ] {
        assert_eq!(decide(command), Verdict::Allow, "{command}");
    }
}

#[test]
fn a_blocker_is_linked_as_blocking_the_issue_it_names() {
    let payload = json!({
        "tool_name": "Bash",
        "hook_event_name": "PostToolUse",
        "tool_input": {"command": "gh issue create -R tompassarelli/north --title t --body 'Blocks: north#316'"},
        "tool_response": {"stdout": "https://github.com/tompassarelli/north/issues/330\n"},
    });
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    let mut linked = Vec::new();
    let verdict = link_with(&input, |new, blocked| {
        linked.push((new.clone(), blocked.clone()));
        Some(())
    });
    assert_eq!(
        linked,
        [(("tompassarelli/north".to_owned(), 330), ("tompassarelli/north".to_owned(), 316))]
    );
    assert!(matches!(verdict, Verdict::Advise(note) if note.contains("north#330 as blocking")));
}

#[test]
fn an_issue_without_a_blocks_line_is_not_linked() {
    let payload = json!({
        "tool_name": "Bash",
        "hook_event_name": "PostToolUse",
        "tool_input": {"command": "gh issue create --title t --body 'plain'"},
        "tool_response": {"stdout": "https://github.com/tompassarelli/north/issues/331\n"},
    });
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    assert_eq!(link_with(&input, |_, _| panic!("linked")), Verdict::Allow);
}

#[test]
fn a_split_or_follow_up_of_the_agents_own_claimed_issue_is_refused_with_the_command_that_adds_boxes() {
    for command in [
        "gh issue create --title 'Port summary' --body 'Split of north#316'",
        "gh issue create -R tompassarelli/north --title 'Port summary' --body 'Follow-up of #316'",
        "gh issue create --title 'Port summary' --body-file - <<'EOF'\nSome text\nSplit of tompassarelli/north#316\nEOF",
    ] {
        let reason = denial(decide(command));
        assert!(reason.contains("gh issue edit 316 -R tompassarelli/north --body-file"), "{reason}");
        assert!(reason.contains("Blocks:"), "{reason}");
    }
    for command in [
        "gh issue create --title 'Port summary' --body-file - <<'EOF'\nBlocks: north#1\nSplit of north#316\nEOF",
        "gh issue create --title 'Port summary' --body-file - <<'EOF'\nNotes\n- **Follow-up of** #316\nEOF",
        "gh issue create --title 'Port summary' --body-file - <<'EOF'\nNotes\n> Follow-up to north#316\nEOF",
    ] {
        let reason = denial(decide(command));
        assert!(reason.contains("gh issue edit 316 -R tompassarelli/north --body-file"), "{reason}");
    }
}

#[test]
fn a_split_of_someone_elses_issue_is_not_the_agents_own_task() {
    assert_eq!(
        decide("gh issue create --title 'Port summary' --body 'Follow-up of north#250'"),
        Verdict::Allow
    );
    assert_eq!(
        decide("gh issue create --title 'Port summary' --body 'Follow-up of other/north#316'"),
        Verdict::Allow
    );
}

#[test]
fn a_title_closely_matching_an_open_issue_is_refused_naming_it() {
    let reason = denial(decide(
        "gh issue create --title 'Make open issue count measure open outcomes in filing guard' --body b",
    ));
    assert!(reason.contains("tompassarelli/north#300"), "{reason}");
    let reason = denial(decide("gh issue create --title 'Harness failing units on whiterabbit' --body b"));
    assert!(reason.contains("north#301"), "{reason}");
    assert!(reason.contains("gh issue edit 301"), "{reason}");
}

#[test]
fn a_distinct_title_and_short_titles_pass() {
    for command in [
        "gh issue create --title 'Cargo registry mirror is unreachable' --body b",
        "gh issue create --title 'Harness units' --body b",
    ] {
        assert_eq!(decide(command), Verdict::Allow, "{command}");
    }
    for command in [
        "gh issue create --title 'Harness failing units on bob' --body b",
        "gh issue create --title 'Port summary 2 to Rust' --body b",
    ] {
        assert_eq!(decide(command), Verdict::Allow, "{command}");
    }
}

#[test]
fn rolling_alarm_issues_are_edited_not_refused_and_other_owners_and_commands_pass() {
    for command in [
        "gh issue edit 301 -R tompassarelli/north --body 'Split of north#316'",
        "gh issue comment 301 --body 'Split of north#316'",
        "echo gh issue create --title 'Harness failing units on whiterabbit'",
        "gh issue create -R someone/else --title 'Harness failing units on whiterabbit'",
    ] {
        assert_eq!(decide(command), Verdict::Allow, "{command}");
    }
}
