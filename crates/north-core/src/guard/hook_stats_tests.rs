use serde_json::{Value, json};

use super::hook_events::events;
use super::hook_rules::{RULES, classify};
use super::hook_stats::{Fire, compute, render};

fn call(id: &str, tool: &str) -> Value {
    json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": tool, "input": {}}]}})
}

fn refused(id: &str, text: &str, at: &str) -> Value {
    json!({"type": "user", "timestamp": at, "message": {"content": [{"type": "tool_result", "tool_use_id": id, "is_error": true, "content": text}]}})
}

fn attachment(kind: &str, id: &str, body: Value) -> Value {
    let mut attachment = json!({"type": kind, "hookName": "PreToolUse:Bash", "hookEvent": "PreToolUse", "toolUseID": id});
    for (key, value) in body.as_object().unwrap() {
        attachment[key] = value.clone();
    }
    json!({"type": "attachment", "timestamp": "2026-11-03T10:00:00Z", "attachment": attachment})
}

fn verdicts(rows: &[Value]) -> Vec<(String, String, String, i64)> {
    rows.iter().map(|row| (row["tool_use"].as_str().unwrap().to_owned(), row["verdict"].as_str().unwrap().to_owned(), row["rule"].as_str().unwrap().to_owned(), row["turn"].as_i64().unwrap())).collect()
}

#[test]
fn transcript_records_become_refusals_rewrites_and_advisories() {
    let rewrite_note = "modern-search-guard: rewrote a recursive grep or find tree search to rg or fd before it ran.";
    let stdout = json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": {"command": "rg x"}, "additionalContext": rewrite_note}}).to_string();
    let lines = vec![
        call("t1", "Bash"),
        refused("t1", "PreToolUse:Bash hook error: BLOCKED: blind staging refused (git add) — enumerate the paths", "2026-11-03T10:00:01Z"),
        call("t2", "Bash"),
        refused("t2", "<tool_use_error>Blocked: sleep 100 followed by: cat out</tool_use_error>", "2026-11-03T10:00:02Z"),
        call("t3", "Bash"),
        attachment("hook_additional_context", "t3", json!({"content": [rewrite_note]})),
        attachment("hook_success", "t3", json!({"stdout": stdout})),
        call("t4", "Agent"),
        refused("t4", "PreToolUse:Agent hook error: [/etc/hooks/north-hook.sh]: tripwire: raw 'git push' — use safe-push", "2026-11-03T10:00:04Z"),
        call("t5", "Bash"),
        attachment("hook_additional_context", "t5", json!({"content": ["Heavy command routed through machine-capacity admission."]})),
        json!({"type": "api-request-blob", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "PreToolUse:Bash hook error: blind staging refused"}]}}),
        refused("t6", "InputValidationError: Monitor failed", "2026-11-03T10:00:06Z"),
        refused("t7", "Exit code 1\nsomething said hook error: but not a hook", "2026-11-03T10:00:07Z"),
    ];
    let rows = events("s1", &lines);
    let expected = [
        ("t1", "refusal", "blind-stage", 1),
        ("t2", "refusal", "claude-sleep", 2),
        ("t3", "rewrite", "modern-search", 3),
        ("t4", "refusal", "tripwire.push", 4),
        ("t5", "advisory", "heavy-command", 5),
    ];
    assert_eq!(verdicts(&rows), expected.map(|(id, verdict, rule, turn)| (id.to_owned(), verdict.to_owned(), rule.to_owned(), turn)).to_vec());
    assert!(rows.iter().all(|row| row["session"] == "s1" && row["type"] == "hook"));
    assert_eq!(rows[3]["tool"], "Agent");
}

#[test]
fn guard_messages_map_to_stable_rule_ids() {
    let cases = [
        ("Workers never block in the foreground on safe-push: the lead's messages", "worker-wait"),
        ("This Bash command would write inside the PRIMARY checkout of north (/home/tom/code/north/main).", "launch-critical"),
        ("BLOCKED: unmanaged background job would detach an agent child from its owner.", "session-kill"),
        ("Missing Category: declare Category and Spec/Scope/Surface/Verify in the worker brief", "worker-tier.missing-facet"),
        ("Scope: single-repo is not a facet value; use one of one-file | module", "worker-tier.facet-value"),
        ("No. full Lua5 suite belongs on the farm. Run on the farm: bun wisp farm", "heavy-command.farm"),
        ("cargo build requires capacity admission; use machine-capacity run", "heavy-command"),
        ("tripwire: unguarded variable as a recursive-delete target ('$dir')", "tripwire.delete"),
        ("tripwire: gh api write to the GitHub git data or contents API", "tripwire"),
        ("BLOCKED: `rg` would perform recursive search rooted at a repository container", "resource-safe-search"),
        ("Hold: claude:a workers 4/4 allowed by measured pace.", "spawn-capacity"),
        ("an unrecognised refusal", "other"),
    ];
    for (text, rule) in cases {
        assert_eq!(classify(text), rule, "{text}");
    }
    let mut ids: Vec<&str> = RULES.iter().map(|rule| rule.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), RULES.len());
}

fn fire(session: &str, verdict: &str, rule: &str, at: i64, turn: i64) -> Fire {
    Fire { session: session.into(), verdict: verdict.into(), rule: rule.into(), at, turn }
}

#[test]
fn stats_count_repeats_and_list_candidates() {
    let now = 100 * 86_400;
    let day = 86_400;
    let fires = vec![
        fire("a", "refusal", "blind-stage", now - day, 3),
        fire("a", "refusal", "blind-stage", now - day, 7),
        fire("a", "refusal", "blind-stage", now - day, 9),
        fire("b", "refusal", "blind-stage", now - day, 2),
        fire("b", "refusal", "blind-stage", now - day, 4),
        fire("c", "refusal", "blind-stage", now - day, 1),
        fire("a", "rewrite", "session-kill", now - day, 5),
        fire("b", "refusal", "session-kill", now - day, 6),
        fire("a", "advisory", "heavy-command", now - 20 * day, 1),
        fire("c", "refusal", "git-stash", now - 40 * day, 1),
    ];
    let stats = compute(&fires, now - 7 * day, now);
    let blind = &stats.rules["blind-stage"];
    assert_eq!((blind.fires, blind.sessions, blind.refusals, blind.repeats), (6, 3, 6, 2));
    assert_eq!(blind.median_lost, Some(4.0));
    let kill = &stats.rules["session-kill"];
    assert_eq!((kill.rewrites, kill.refusals, kill.repeats, kill.median_lost), (1, 1, 0, None));
    assert!(!stats.rules.contains_key("heavy-command"));
    assert_eq!(stats.auto_fix, vec!["session-kill".to_owned()]);
    assert_eq!(stats.skill_text, vec!["blind-stage".to_owned()]);
    assert!(stats.prune.contains(&"git-stash"));
    assert!(!stats.prune.contains(&"heavy-command"));
    assert!(!stats.prune.contains(&"claude-sleep"));
    let text = render(&stats, "7d");
    assert!(text.contains("blind-stage: 2 sessions repeated it; line that should have prevented it: repo-safety: stage enumerated paths"));
    assert!(text.contains("session-kill: 1 refused, 1 rewritten (50% rewritten)"));
}
