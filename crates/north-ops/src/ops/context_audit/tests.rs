use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::scan::{Kind, Role, named_paths, path_key};
use super::{audit_all, duration, report, session_files};

const LEAD: &str = "0b1c2d3e-0000-4000-8000-000000000001";
const GROUNDING: &str = "Grounding (auto-retrieved for this message; terms: x). Read ~/code/north/main/harness/skills/workers/SKILL.md and docs/features.md and crates/a/b.rs before answering.";
const FILE_SKILL: &str = "file-skill: editing /w/src/a.rs loads the rust-development skill (directory /s/rust-development); its rules apply.\n\nbody";
const SKILL_BODY: &str = "Base directory for this skill: /home/u/.local/state/skills/verification\n\n# Verification";
const SKILL_READ: &str = "---\nname: planning\n---\nbody text";
const REFERENCE_READ: &str = "reference text here";
const ADVISORY: &str = "modern-search-guard: rewrote a search";

fn hook(uuid: &str, parent: &str, event: &str, text: &str) -> Value {
    json!({"type": "attachment", "uuid": uuid, "parentUuid": parent, "attachment": {"type": "hook_additional_context", "hookEvent": event, "content": [text]}})
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
}

fn tool_result(id: &str, text: &str) -> Value {
    json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text}]}})
}

fn transcript() -> Vec<Value> {
    vec![
        json!({"type": "user", "uuid": "u1", "message": {"content": "please do the thing"}}),
        tool_use("t0", "Read", json!({"file_path": "/home/u/code/north/worktrees/x/crates/a/b.rs"})),
        tool_result("t0", "early read"),
        hook("h1", "u1", "UserPromptSubmit", GROUNDING),
        json!({"type": "attachment", "uuid": "q1", "attachment": {"type": "queued_command", "prompt": "<task-notification>\n<task-id>x</task-id>"}}),
        hook("h2", "q1", "UserPromptSubmit", GROUNDING),
        hook("h3", "h2", "PreToolUse", FILE_SKILL),
        hook("h4", "h3", "PostToolUse", ADVISORY),
        tool_use("t1", "Skill", json!({"skill": "verification"})),
        tool_result("t1", "Launching skill: verification"),
        json!({"type": "user", "isMeta": true, "sourceToolUseID": "t1", "message": {"content": [{"type": "text", "text": SKILL_BODY}]}}),
        tool_use("t2", "Read", json!({"file_path": "/home/u/code/north/main/harness/skills/planning/SKILL.md"})),
        tool_result("t2", SKILL_READ),
        tool_use("t3", "Read", json!({"file_path": "/home/u/code/north/main/harness/skills/planning/references/brief.md"})),
        tool_result("t3", REFERENCE_READ),
        tool_use("t4", "Bash", json!({"command": "rg -n foo docs/features.md"})),
        tool_result("t4", "ordinary output is not injected context"),
        json!({"type": "api-request-blob", "content": [{"type": "text", "text": GROUNDING}]}),
    ]
}

fn write(path: &Path, records: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = records.iter().map(|record| format!("{record}\n")).collect();
    fs::write(path, text).unwrap();
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

#[test]
fn every_attribution_rule_lands_in_its_source() {
    let root = tempfile::tempdir().unwrap();
    let lead = root.path().join(format!("-p/{LEAD}.jsonl"));
    let worker = root.path().join(format!("-p/{LEAD}/subagents/agent-a1.jsonl"));
    write(&lead, &transcript());
    write(&worker, &[hook("w1", "", "PreToolUse", ADVISORY)]);
    let files = session_files(&[root.path().to_path_buf()], Duration::from_secs(3600), None).unwrap();
    let audits = audit_all(&files, 400);
    assert_eq!(audits.len(), 2);
    let (lead, worker) = (&audits[0], &audits[1]);
    assert_eq!((lead.role, worker.role), (Role::Lead, Role::Worker));
    assert_eq!(lead.session, LEAD);
    assert_eq!(lead.chars[&Kind::Startup], 400);
    assert_eq!(worker.chars[&Kind::Startup], 400);
    assert_eq!(worker.chars[&Kind::OtherHook], chars(ADVISORY));
    assert_eq!(lead.chars[&Kind::Grounding], 2 * chars(GROUNDING));
    assert_eq!(lead.chars[&Kind::OtherHook], chars(ADVISORY));
    assert_eq!(lead.skills["rust-development"], chars(FILE_SKILL));
    assert_eq!(lead.skills["verification"], chars("Launching skill: verification") + chars(SKILL_BODY));
    assert_eq!(lead.skills["planning"], chars(SKILL_READ));
    assert_eq!(lead.chars[&Kind::SkillBody], lead.skills.values().sum::<usize>());
    assert_eq!(lead.references["planning/references/brief.md"], chars(REFERENCE_READ));
    assert_eq!(lead.chars[&Kind::SkillReference], chars(REFERENCE_READ));
    assert_eq!(lead.total(), 400 + 2 * chars(GROUNDING) + chars(ADVISORY) + lead.chars[&Kind::SkillBody] + chars(REFERENCE_READ));
    assert_eq!(lead.grounding.injections, 2);
    assert_eq!(lead.grounding.notification_injections, 1);
    assert_eq!(lead.grounding.named, 6);
    assert_eq!(lead.grounding.read, 2);
}

#[test]
fn grounding_paths_normalize_across_checkouts() {
    assert_eq!(named_paths(GROUNDING), vec!["harness/skills/workers/SKILL.md", "docs/features.md", "crates/a/b.rs"]);
    assert_eq!(path_key("/home/u/code/north/worktrees/lane/docs/x.md"), "docs/x.md");
    assert_eq!(path_key("/home/u/.local/state/agents/pace.json"), "~/.local/state/agents/pace.json");
    assert!(named_paths("see https://example.com/a/b.md").is_empty());
}

#[test]
fn session_filter_and_since_select_files() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(format!("-p/{LEAD}.jsonl")), &transcript());
    write(&root.path().join("-p/1b1c2d3e-0000-4000-8000-000000000002.jsonl"), &transcript());
    let roots = [root.path().to_path_buf()];
    assert_eq!(session_files(&roots, Duration::from_secs(3600), Some("0b1c")).unwrap().len(), 1);
    assert_eq!(session_files(&roots, Duration::from_secs(3600), None).unwrap().len(), 2);
    assert_eq!(duration("7d"), Some(Duration::from_secs(7 * 86_400)));
    assert_eq!(duration("30m"), Some(Duration::from_secs(1800)));
    assert_eq!(duration("7w"), None);
}

#[test]
fn summary_reports_percentiles_top_skills_and_precision() {
    assert_eq!(report::percentile(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 0.5), 5);
    assert_eq!(report::percentile(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 0.9), 9);
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(format!("-p/{LEAD}.jsonl")), &transcript());
    let files = session_files(&[root.path().to_path_buf()], Duration::from_secs(3600), None).unwrap();
    let audits = audit_all(&files, 400);
    let text = report::summary(&audits, "7d", 100, Duration::from_secs(1));
    assert!(text.contains("1 sessions (1 leads, 0 workers)"), "{text}");
    assert!(text.contains("grounding precision: 33.3% (2 of 6 named paths later read, over 2 injections)"), "{text}");
    assert!(text.contains("on background task notifications: 1 of 2 (50.0%)"), "{text}");
    assert!(text.lines().any(|line| line.trim_start().starts_with("rust-development")), "{text}");
    let parsed: Value = serde_json::from_str(&report::json(&audits, "7d", 100, Duration::from_secs(1))).unwrap();
    assert_eq!(parsed["sessions"][0]["role"], "lead");
    assert_eq!(parsed["sessions"][0]["tokens"]["startup"], 100);
}
