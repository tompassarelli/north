use super::*;
use serde_json::json;

struct Fake {
    claimed: Vec<&'static str>,
    open: Vec<(u64, &'static str)>,
    brief: Option<&'static str>,
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

    fn worker_brief(&mut self) -> Option<String> {
        self.brief.map(str::to_owned)
    }
}

fn world() -> Fake {
    Fake {
        claimed: vec!["north#316"],
        open: vec![
            (300, "Make the open-issue count measure open outcomes in the filing guard"),
            (301, "Harness failing units on whiterabbit"),
        ],
        brief: None,
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

const BRIEF: &str = "Item: north#316 Delete the calibration helpers\nCategory: tooling\nDone when: bin/threads defines no calibration helper and `north threads summary` prints the closure table\n";

fn worker_decide(command: &str) -> Verdict {
    decide_with(&pre(command), &mut Fake { brief: Some(BRIEF), ..world() })
}

#[test]
fn a_worker_issue_naming_no_blocked_box_or_measured_defect_is_refused_with_the_in_lane_rule() {
    for command in [
        "gh issue create --title 'Port the summary table to a shared module' --body 'Would be cleaner.'",
        "gh issue create --title 'Idea: cache the calibration rows' --body-file - <<'EOF'\nFound while porting north#316.\nCategory: tooling\nEOF",
    ] {
        let reason = denial(worker_decide(command));
        assert!(reason.contains("Blocks: repo#N") && reason.contains("Defect:"), "{reason}");
        assert!(reason.contains("20 minutes") && reason.contains("Ideas:"), "{reason}");
    }
}

#[test]
fn a_worker_issue_for_a_blocker_a_measured_defect_or_its_own_launch_item_passes() {
    for command in [
        "gh issue create --title 'Cargo registry mirror is unreachable' --body 'Blocks: north#316'",
        "gh issue create --title 'Ledger write contention drops runs' --body-file - <<'EOF'\n**Defect:** `north fleet list --json` exited 1 three times in 20 runs: database is locked\nEOF",
        "gh issue create --title 'Delete the calibration helpers' --body-file - <<'EOF'\nDone when: bin/threads defines no calibration helper and\n`north threads summary` prints the closure table\nEOF",
    ] {
        assert_eq!(worker_decide(command), Verdict::Allow, "{command}");
    }
}

#[test]
fn a_session_without_a_worker_brief_files_without_a_blocks_or_defect_line() {
    assert_eq!(decide("gh issue create --title 'Idea: cache the calibration rows' --body 'Would be faster.'"), Verdict::Allow);
}

#[test]
fn a_worker_brief_is_read_only_from_a_fleet_session_transcript_whose_prompt_has_a_done_when_line() {
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("s.jsonl");
    let line = |text: &str| json!({"type": "user", "message": {"role": "user", "content": text}}).to_string();
    std::fs::write(&transcript, format!("{}\n{}\n", json!({"type": "summary"}), line(BRIEF))).unwrap();
    let live = |session: Option<&str>| Live { cwd: None, transcript: Some(transcript.clone()), fleet_session: session.map(str::to_owned) };
    assert_eq!(live(Some("issue-intake")).worker_brief().as_deref(), Some(BRIEF));
    assert_eq!(live(None).worker_brief(), None);
    std::fs::write(&transcript, format!("{}\n", line("You are taking over as Tom's proxy now."))).unwrap();
    assert_eq!(live(Some("proxy")).worker_brief(), None);
}

#[test]
fn a_bold_or_listed_blocks_line_counts_and_a_listed_done_when_marks_a_worker() {
    for command in [
        "gh issue create --title 'Cargo mirror down' --body '**Blocks:** north#316'",
        "gh issue create --title 'Cargo mirror down' --body '- Blocks: north#316'",
    ] {
        assert_eq!(worker_decide(command), Verdict::Allow, "{command}");
    }
    let listed = "Item: north#1 x\nCategory: tooling\nDone when:\n- [ ] `cargo test` passes with the ledger fixture loaded\nETA: 30 min\n";
    let mut world = Fake { brief: Some(listed), ..world() };
    let idea = pre("gh issue create --title 'Cache the rows' --body 'Would be faster.'");
    assert!(matches!(decide_with(&idea, &mut world), Verdict::Deny(_)));
    let own = pre("gh issue create --title 'x' --body-file - <<'EOF'\n## Done when\n\n- [ ] (local test) `cargo test` passes with the ledger fixture loaded\nEOF");
    assert_eq!(decide_with(&own, &mut world), Verdict::Allow);
}

#[test]
fn an_unreadable_worker_body_is_refused_with_how_to_make_it_readable() {
    let reason = denial(worker_decide("b='Blocks: north#316'; gh issue create --title T --body \"$b\""));
    assert!(reason.contains("heredoc"), "{reason}");
}
