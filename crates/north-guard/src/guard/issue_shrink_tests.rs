use super::*;
use serde_json::json;

struct Fake {
    body: String,
}

impl World for Fake {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        Some(named.unwrap_or("tompassarelli/north").to_owned())
    }

    fn body(&mut self, _: &str, _: u64) -> Option<String> {
        Some(self.body.clone())
    }
}

fn verdict(dir: &std::path::Path, command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": dir, "tool_input": {"command": command}});
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    refuse(&input, &mut Fake { body: "x".repeat(1000) })
}

#[test]
fn an_issue_edit_that_empties_or_halves_the_body_is_refused_and_an_ordinary_edit_or_a_named_shrink_passes_north_706() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empty.md"), "").unwrap();
    std::fs::write(dir.path().join("short.md"), "y".repeat(400)).unwrap();
    std::fs::write(dir.path().join("grown.md"), "y".repeat(1200)).unwrap();
    let edit = |file: &str| format!("gh issue edit 706 -R tompassarelli/north --body-file {file}");
    assert!(matches!(verdict(dir.path(), &edit("empty.md")), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), &edit("short.md")), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), "gh issue edit 706 -R tompassarelli/north --body ''"), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), &edit("grown.md")), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), &format!("{} --allow-shrink 'superseded by north#900'", edit("empty.md"))), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), "gh issue edit 706 -R tompassarelli/north --title 'New title'"), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), &format!("script > empty.md && {}", edit("empty.md"))), Verdict::Allow));
}
