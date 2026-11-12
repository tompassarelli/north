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

fn verdict(command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": "/tmp", "tool_input": {"command": command}});
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    refuse_close(&input, &mut Fake { body: "- [x] Shipped the guard.\n".to_owned() })
}

#[test]
fn closes_an_issue_the_same_command_edits_or_drops_as_not_planned() {
    assert!(matches!(verdict("gh issue close 5 -R tompassarelli/north"), Verdict::Deny(_)));
    assert!(matches!(verdict("gh issue edit 6 -R tompassarelli/north --body-file /tmp/b.md && gh issue close 5 -R tompassarelli/north"), Verdict::Deny(_)));
    assert!(matches!(verdict("gh issue edit 5 -R tompassarelli/north --body-file /tmp/b.md && gh issue close 5 -R tompassarelli/north"), Verdict::Allow));
    assert!(matches!(verdict("gh issue close 5 -R tompassarelli/north -r \"not planned\""), Verdict::Allow));
}
