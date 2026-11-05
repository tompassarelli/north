use super::*;
use serde_json::json;

const TERM: &str = "orange-bicycle";

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
    dir
}

#[test]
fn refuses_a_deny_listed_term_in_every_place_a_gh_write_carries_text() {
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
}

