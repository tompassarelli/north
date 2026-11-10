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
