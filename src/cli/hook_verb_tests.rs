use serde_json::{Value, json};

use crate::guard::proxy_role::streak_verdict as verdict;
use north_guard::hook_input::Verdict;

const LIMIT: usize = 15;

const LAPTOP_LOAD_READS: [&str; 16] = [
    "uptime",
    "cat /proc/loadavg",
    "ps -eo pid,pcpu,comm --sort=-pcpu | head -30",
    "systemd-cgtop -b -n 1 | head -20",
    "systemctl --user show agent.slice -p CPUQuotaPerSecUSec",
    "systemctl --user status north-land@north.service",
    "journalctl --user -u north-land@north.service -n 50",
    "north capacity status",
    "north capacity ledger",
    "cat /sys/class/power_supply/BAT0/status",
    "sensors | rg Package",
    "rg -n aggregateCpuLimit ~/.local/state/agents/capacity-mode.json",
    "north fleet list",
    "systemctl --user list-units 'north-*' --state=running",
    "rg -n cores ~/.local/state/agents/machine-capacity-usage.jsonl | tail -20",
    "git -C ~/code/north/main log --oneline -5 -- crates/north-capacity",
];

fn prompt(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
}

fn bash(id: &str, command: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}}]}}).to_string()
}

fn result(id: &str) -> String {
    json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": "ok"}]}}).to_string()
}

fn turn(prompt_text: &str, commands: &[&str]) -> Vec<String> {
    let mut lines = vec![prompt(prompt_text)];
    for (at, command) in commands.iter().enumerate() {
        lines.push(bash(&format!("t{at}"), command));
        lines.push(result(&format!("t{at}")));
    }
    lines
}

fn hook(id: &str, command: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": id, "tool_input": {"command": command}})
}

fn judged(lines: &[String], last: &Value) -> Verdict {
    verdict(&super::read_streak(&lines.join("\n"), last), LIMIT)
}

#[test]
fn a_proxy_turn_reading_past_the_limit_on_one_subject_is_stopped_with_its_brief() {
    let question = "why is the laptop at load 87 on battery while I'm using it?";
    let lines = turn(question, &LAPTOP_LOAD_READS[..LIMIT]);
    assert_eq!(judged(&lines, &hook("t14", LAPTOP_LOAD_READS[14])), Verdict::Allow, "the limit itself still runs");
    let Verdict::Deny(reason) = judged(&lines, &hook("t15", LAPTOP_LOAD_READS[15])) else { panic!("the 16th read call went on") };
    assert!(reason.contains("16 read-only calls") && reason.contains(question) && reason.contains("north work brief new") && reason.contains("north fleet launch"), "{reason}");

    let mut handed_off = turn(question, &LAPTOP_LOAD_READS[..8]);
    handed_off.extend(turn_tail(&["north fleet launch --task north#834 --brief /tmp/b"], 100));
    handed_off.extend(turn_tail(&LAPTOP_LOAD_READS[8..15], 200));
    assert_eq!(judged(&handed_off, &hook("t300", LAPTOP_LOAD_READS[15])), Verdict::Allow, "a launch starts a new streak");

    let mut gated = turn(question, &LAPTOP_LOAD_READS[..LIMIT]);
    gated.extend(turn_tail(&["north gate open --question 'cap the lander?' --option yes --option no --recommend 1"], 100));
    assert_eq!(judged(&gated, &hook("t300", LAPTOP_LOAD_READS[15])), Verdict::Allow, "a gate starts a new streak");

    let mut next_prompt = turn(question, &LAPTOP_LOAD_READS[..LIMIT]);
    next_prompt.extend(turn("and the cost table?", &[]));
    assert_eq!(judged(&next_prompt, &hook("t300", LAPTOP_LOAD_READS[15])), Verdict::Allow, "Tom's next message starts a new turn");

    let mut subagent = turn(question, &LAPTOP_LOAD_READS[..LIMIT]);
    subagent.push(json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "tool_use", "id": "s1", "name": "Bash", "input": {"command": "uptime"}}]}}).to_string());
    assert_eq!(judged(&subagent, &hook("t14", LAPTOP_LOAD_READS[14])), Verdict::Allow, "an Explore subagent's reads are not the proxy's");
}

fn turn_tail(commands: &[&str], first: usize) -> Vec<String> {
    commands.iter().enumerate().flat_map(|(at, command)| [bash(&format!("t{}", first + at), command), result(&format!("t{}", first + at))]).collect()
}

#[test]
fn sends_between_reads_do_not_refill_one_messages_looks() {
    let budgets = crate::guard::proxy_role::Budgets { looks: 25, reach: 8 };
    let question = "why is the laptop at load 87?";
    let mut lines = vec![prompt(question)];
    for at in 0..26 {
        lines.push(bash(&format!("r{at}"), &format!("uptime # {at}")));
        lines.push(result(&format!("r{at}")));
        if at % 5 == 4 {
            lines.push(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": format!("m{at}"), "name": "SendMessage", "input": {"to": "w", "message": "go"}}]}}).to_string());
        }
    }
    let judge = |lines: &[String], last: &Value| crate::guard::proxy_role::budget_verdict(&super::read_streak(&lines.join("\n"), last), &budgets);
    assert_eq!(judge(&lines[..lines.len() - 2], &hook("r24", "uptime # 24")), Verdict::Allow, "the 25th look runs");
    assert!(matches!(judge(&lines, &hook("r25", "uptime # 25")), Verdict::Deny(reason) if reason.contains("26 read-only calls")), "the 26th look is stopped");
    let mut next = lines.clone();
    next.push(prompt("and the cost table?"));
    assert_eq!(judge(&next, &hook("r26", "uptime")), Verdict::Allow, "Tom's next message refills the budget");

    let ssh: Vec<String> = std::iter::once(prompt(question)).chain((0..9).flat_map(|at| [bash(&format!("s{at}"), &format!("ssh host{at} uptime")), result(&format!("s{at}"))])).collect();
    assert!(matches!(judge(&ssh, &hook("s9", "ssh host9 uptime")), Verdict::Deny(reason) if reason.contains("ssh (10)")), "ten ssh reads exceed the reach budget of eight");
    let source = vec![prompt(question), bash("a", "rg -n pace ~/code/north/main/crates/north-pace"), result("a")];
    assert_eq!(judge(&source, &hook("b", "rg -n lanes ~/code/north/main/crates/north-lanes")), Verdict::Allow, "two source reads are inside the budget");
}

#[test]
fn the_recorded_load_trace_is_counted_as_its_fixture_says() {
    let trace: Value = serde_json::from_str(include_str!("../../crates/north-guard/src/guard/fixtures/proxy_load_trace_2026_11_11.json")).unwrap();
    for message in trace["messages"].as_array().unwrap() {
        let mut lines = vec![prompt("a question from Tom")];
        for (at, call) in message["calls"].as_array().unwrap().iter().enumerate() {
            let command = call["command"].as_str().unwrap();
            let reads = super::read_streak(&lines.join("\n"), &hook(&format!("c{at}"), command)).message;
            let counted = (reads.looks as u64, reads.source as u64, reads.remote as u64, reads.looking);
            let recorded = (call["looks"].as_u64().unwrap(), call["source"].as_u64().unwrap(), call["remote"].as_u64().unwrap(), call["looking"].as_bool().unwrap());
            assert_eq!(counted, recorded, "{command}");
            lines.push(bash(&format!("c{at}"), command));
            lines.push(result(&format!("c{at}")));
        }
    }
}
