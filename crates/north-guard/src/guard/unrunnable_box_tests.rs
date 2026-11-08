use super::*;

struct Recorder {
    current: &'static str,
    rows: Vec<serde_json::Value>,
}

impl World for Recorder {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        named.map(str::to_owned)
    }

    fn current(&mut self, _: &str, _: &str) -> Option<String> {
        Some(self.current.to_owned())
    }

    fn record(&mut self, row: serde_json::Value) {
        self.rows.push(row);
    }
}

fn shell(command: &str) -> HookInput {
    HookInput::parse(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": command}}).to_string().as_bytes()).unwrap()
}

#[test]
fn an_edit_logs_only_its_new_box_without_a_runnable_done_when_and_refuses_nothing() {
    let held = "- [ ] Remove the issue-level reading, not before 2026-11-15\n- [ ] An old box with no command";
    let edited = format!(
        "{held}\n- [ ] A new box a worker can only tick itself\n- [ ] A new box with its check. Done when: `cargo test -p north-guard unrunnable` passes\n- [ ] A decision for Tom. Needs: Tom\n- [x] A ticked box with no command"
    );
    let mut world = Recorder { current: held, rows: Vec::new() };
    shadow(&shell(&format!("gh issue edit 872 -R tompassarelli/north --body-file - <<'EOF'\n{edited}\nEOF")), &mut world);
    let logged: Vec<(&str, &str, &str)> = world.rows.iter().map(|row| (row["verb"].as_str().unwrap(), row["issue"].as_str().unwrap(), row["box"].as_str().unwrap())).collect();
    assert_eq!(logged, [("edit", "872", "A new box a worker can only tick itself")]);
    let mut world = Recorder { current: "", rows: Vec::new() };
    shadow(&shell("gh issue create -R tompassarelli/north --title 'A filing' --body '- [ ] An old box with no command'"), &mut world);
    assert_eq!(world.rows.len(), 1, "a created issue logs its unrunnable box");
}
