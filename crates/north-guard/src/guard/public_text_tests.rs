use super::*;
use serde_json::json;

const TERM: &str = "orange-bicycle";
const KIND_TEXT: &str = "walnut-lantern";

fn input(command: &str, cwd: &str) -> HookInput {
    let payload = json!({"tool_name": "Bash", "cwd": cwd, "tool_input": {"command": command}});
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

fn decide(command: &str, cwd: &str, private: bool) -> Verdict {
    decide_with(
        &input(command, cwd),
        |text| {
            text.contains(TERM)
                .then(|| "private-fixture".to_owned())
                .into_iter()
                .collect()
        },
        |_, _| private,
        |text| match text {
            text if text.contains("judge-down") => Err("no claude account answered".to_owned()),
            text if text.contains(KIND_TEXT) => Ok(vec![Finding { kind: "fixture-kind".into(), span: KIND_TEXT.into() }]),
            _ => Ok(Vec::new()),
        },
        |write| write.edits.as_ref().map(|_| format!("- [ ] landed\nthe {KIND_TEXT} plan\n")),
    )
}

fn refused(command: &str, cwd: &str) -> bool {
    match decide(command, cwd, false) {
        Verdict::Deny(reason) => {
            assert!(reason.contains("private-fixture"), "{reason}");
            assert!(!reason.contains(TERM), "{reason}");
            true
        }
        Verdict::Allow => false,
        other => panic!("{other:?}"),
    }
}

fn scratch() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dirty.md"), format!("the {TERM} again\n")).unwrap();
    std::fs::write(dir.path().join("clean.md"), "landed and verified\n").unwrap();
    std::fs::write(dir.path().join("ticked.md"), format!("- [x] landed\nthe {KIND_TEXT} plan\n")).unwrap();
    std::fs::write(dir.path().join("grown.md"), format!("- [x] landed\nthe {KIND_TEXT} plan\nthe {KIND_TEXT} again\n")).unwrap();
    dir
}

#[test]
fn refuses_a_deny_listed_term_and_a_private_kind_in_every_place_a_gh_write_carries_text() {
    let dir = scratch();
    let cwd = dir.path().to_str().unwrap();
    for command in [
        format!("gh issue comment 5 --body 'the {TERM} again'"),
        format!("gh issue comment 5 -b \"the {TERM} again\""),
        format!("gh issue comment 5 --body='the {TERM} again'"),
        format!("gh pr comment 5 --body 'the {TERM} again'"),
        format!("gh issue create --title 'the {TERM}' --body ok"),
        "gh pr edit 5 --body-file dirty.md".to_owned(),
        format!("gh issue comment 5 --body-file {cwd}/dirty.md"),
        "gh issue comment 5 -F dirty.md".to_owned(),
        "gh issue comment 5 --body \"$(cat dirty.md)\"".to_owned(),
        format!("gh issue comment 5 --body-file - <<'EOF'\nthe {TERM}\nEOF"),
        format!("gh api repos/o/r/issues/5/comments -f body='the {TERM}'"),
        "gh api repos/o/r/issues/5/comments -F body=@dirty.md".to_owned(),
        "gh api repos/o/r/issues/5/comments --input dirty.md".to_owned(),
        format!("cd /x && gh -R o/r issue comment 5 --body 'the {TERM}'"),
        format!("printf '%s' '{TERM}' | gh issue comment 5 --body-file -"),
        format!("body='the {TERM}'; gh issue comment 5 --body \"$body\""),
        format!("gh api -X POST repos/o/r/issues/5/comments -f body='the {TERM}'"),
        "gh issue comment 5 -F - < dirty.md".to_owned(),
    ] {
        assert!(refused(&command, cwd), "{command}");
    }
    let Verdict::Deny(reason) = decide(&format!("gh issue comment 5 --body 'the {KIND_TEXT} plan'"), cwd, false) else { panic!("a private kind passed") };
    assert!(reason.contains("file it in south: fixture-kind"), "{reason}");
    assert!(matches!(decide("gh issue comment 5 --body 'the sandbox keeps the lane writable'", cwd, false), Verdict::Allow));
    assert!(matches!(decide(&format!("gh issue comment 5 --body 'the {KIND_TEXT} plan'"), cwd, true), Verdict::Allow));
    let Verdict::Deny(reason) = decide("gh issue comment 5 --body 'judge-down'", cwd, false) else { panic!("an unjudged text passed") };
    assert!(reason.contains("has no verdict"), "{reason}");
    assert!(matches!(decide("gh issue edit 5 --body-file ticked.md", cwd, false), Verdict::Allow));
    assert!(matches!(decide("gh issue edit 5 --body-file grown.md", cwd, false), Verdict::Deny(_)));
}

#[test]
fn judges_only_the_heredoc_that_feeds_the_gh_command() {
    let script = "cat > /tmp/helper.sh <<'EOF'\necho orange-bicycle\nEOF\n";
    assert!(!refused(&format!("{script}gh issue comment 5 --body fine"), "/tmp"));
    let body = "cat > /tmp/body-n706.md <<'EOF'\nthe orange-bicycle plan\nEOF\n";
    assert!(refused(&format!("{body}gh issue comment 5 --body-file /tmp/body-n706.md"), "/tmp"));
    assert!(refused("gh issue comment 5 --body-file - <<'EOF'\nthe orange-bicycle plan\nEOF\n", "/tmp"));
}

fn sample_rows(rule: &str) -> Vec<(String, String)> {
    let document: toml_edit::DocumentMut = crate::guard::hook_stats::SAMPLES.parse().unwrap();
    document["sample"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .filter(|sample| sample["rule"].as_str() == Some(rule))
        .flat_map(|sample| sample["row"].as_array_of_tables().unwrap().iter())
        .map(|row| (row["command"].as_str().unwrap_or_default().to_owned(), row["label"].as_str().unwrap_or_default().to_owned()))
        .collect()
}

/// Replays the stored 2026-11-08 privacy-judge sample through the current body selection: a stub
/// judge refuses every body, so a row is still refused only when the guard still sends it one.
/// Every row labelled false must now pass; the true rows whose command still shows its gh body
/// must still be refused (north#1041).
#[test]
fn the_2026_11_08_privacy_judge_sample_reads_no_false_refusal_after_the_body_selection_fix_north_1041() {
    let mut false_refused = Vec::new();
    let mut true_refused = 0;
    for (command, label) in sample_rows("privacy-judge") {
        let verdict = decide_with(
            &input(&command, "/tmp"),
            |_| BTreeSet::new(),
            |_, _| false,
            |text| if text.trim().is_empty() { Ok(Vec::new()) } else { Ok(vec![Finding { kind: "fixture-kind".into(), span: "x".into() }]) },
            |_| None,
        );
        let refused = matches!(verdict, Verdict::Deny(_));
        match label.as_str() {
            "false" if refused => false_refused.push(command),
            "true" if refused => true_refused += 1,
            _ => {}
        }
    }
    assert!(false_refused.is_empty(), "false refusals still judged: {false_refused:#?}");
    assert!(true_refused >= 2, "only {true_refused} true rows still refused");
}
