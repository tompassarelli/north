use serde_json::{Value, json};

use north_guard::hook_input::Verdict;

fn prompt(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
}

fn bash(id: &str, command: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}}]}}).to_string()
}

fn result(id: &str) -> String {
    json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": "ok"}]}}).to_string()
}

fn hook(id: &str, command: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": id, "tool_input": {"command": command}})
}

#[test]
fn sends_between_reads_do_not_refill_one_messages_looks() {
    let budgets = crate::guard::coordinator_role::Budgets { looks: 25 };
    let question = "why is the laptop at load 87?";
    let mut lines = vec![prompt(question)];
    for at in 0..26 {
        lines.push(bash(&format!("r{at}"), &format!("uptime # {at}")));
        lines.push(result(&format!("r{at}")));
        if at % 5 == 4 {
            lines.push(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": format!("m{at}"), "name": "SendMessage", "input": {"to": "w", "message": "go"}}]}}).to_string());
        }
    }
    let judge = |lines: &[String], last: &Value| crate::guard::coordinator_role::budget_verdict(&super::read_streak(&lines.join("\n"), last, |_, _| false), &budgets);
    assert_eq!(judge(&lines[..lines.len() - 2], &hook("r24", "uptime # 24")), Verdict::Allow, "the 25th look runs");
    assert!(matches!(judge(&lines, &hook("r25", "uptime # 25")), Verdict::Deny(reason) if reason.contains("26 read-only calls")), "the 26th look is stopped");
    assert_eq!(judge(&lines, &hook("a", "north fleet adopt --name coordinator")), Verdict::Allow, "adopting the window records it, so a spent budget never stops it");
    let mut next = lines.clone();
    next.push(prompt("and the cost table?"));
    assert_eq!(judge(&next, &hook("r26", "uptime")), Verdict::Allow, "Tom's next message refills the budget");
}

#[test]
fn the_recorded_load_trace_is_counted_as_its_fixture_says() {
    let trace: Value = serde_json::from_str(include_str!("../../crates/north-guard/src/guard/fixtures/proxy_load_trace_2026_11_11.json")).unwrap();
    for message in trace["messages"].as_array().unwrap() {
        let mut lines = vec![prompt("a question from Tom")];
        for (at, call) in message["calls"].as_array().unwrap().iter().enumerate() {
            let command = call["command"].as_str().unwrap();
            let reads = super::read_streak(&lines.join("\n"), &hook(&format!("c{at}"), command), |_, _| false).message;
            let counted = (reads.looks as u64, reads.looking);
            let recorded = (call["looks"].as_u64().unwrap(), call["looking"].as_bool().unwrap());
            assert_eq!(counted, recorded, "{command}");
            lines.push(bash(&format!("c{at}"), command));
            lines.push(result(&format!("c{at}")));
        }
    }
    {
        let call = |id: &str, name: &str, input: Value| json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}).to_string();
        let mut lines = vec![prompt("why is the land queue stuck?")];
        for at in 0..15 {
            lines.push(bash(&format!("r{at}"), &format!("uptime # {at}")));
            lines.push(result(&format!("r{at}")));
            if at == 10 {
                lines.push(call("l", "ListAgents", json!({})));
            }
        }
        let judge = |name: &str, input: Value| {
            let hook = json!({"hook_event_name": "PreToolUse", "tool_name": name, "tool_use_id": "next", "tool_input": input});
            crate::guard::coordinator_role::streak_verdict(&super::read_streak(&lines.join("\n"), &hook, |_, _| false), 15)
        };
        for (name, input) in [
            ("Bash", json!({"command": "uptime"})),
            ("Read", json!({"file_path": "/tmp/land.log"})),
            ("Grep", json!({"pattern": "error", "path": "/tmp"})),
            ("Glob", json!({"pattern": "/tmp/*.log"})),
            ("WebFetch", json!({"url": "https://example.com", "prompt": "status"})),
            ("WebSearch", json!({"query": "land queue"})),
        ] {
            assert!(matches!(judge(name, input), Verdict::Deny(reason) if reason.contains("16 read-only calls")), "{name} past the streak runs");
        }
        for (name, input) in [
            ("ListAgents", json!({})),
            ("Monitor", json!({"command": "tail -f /tmp/x"})),
            ("TaskStop", json!({"task_id": "b1"})),
            ("SendMessage", json!({"to": "w", "message": "go"})),
            ("Agent", json!({"subagent_type": "Explore", "prompt": "look"})),
        ] {
            assert_eq!(judge(name, input), Verdict::Allow, "{name} is refused past the streak");
        }
    }
}
