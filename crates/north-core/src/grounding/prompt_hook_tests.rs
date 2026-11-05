use super::*;
use serde_json::Value;
use std::cell::Cell;

const CONTEXT: &str = "Grounding\n- harness/skills/kayak-paddle/SKILL.md\n- ledger:L1";
const PROMPT: &str = r#"{"hook_event_name":"UserPromptSubmit","session_id":"s0","prompt":"plan a kayak paddle trip with the river gauge"}"#;

struct Rig {
    dir: tempfile::TempDir,
    calls: Cell<u32>,
    off: Cell<bool>,
}

impl Rig {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), calls: Cell::new(0), off: Cell::new(false) }
    }

    fn hook(&self, payload: &str, answer: HookReply) -> Option<String> {
        let off = || self.off.get();
        let gate = Gate { guards_off: &off, errors: self.dir.path().join("errors.tsv") };
        decide(payload.as_bytes(), &gate, |_| {
            self.calls.set(self.calls.get() + 1);
            Ok(answer)
        })
    }

    fn errors(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("errors.tsv")).unwrap_or_default()
    }
}

#[test]
fn a_proxy_prompt_gets_the_grounding_block_as_user_prompt_submit_context() {
    let rig = Rig::new();
    let out = rig.hook(PROMPT, HookReply::Context(CONTEXT.into())).unwrap();
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    assert_eq!(value["hookSpecificOutput"]["additionalContext"], CONTEXT);
    assert_eq!(rig.hook(PROMPT, HookReply::Silent), None);
}

#[test]
fn a_subagent_payload_prints_nothing_and_never_reaches_grounding() {
    let rig = Rig::new();
    let subagent = r#"{"hook_event_name":"UserPromptSubmit","agent_id":"w","prompt":"kayak paddle river gauge"}"#;
    assert_eq!(rig.hook(subagent, HookReply::Context(CONTEXT.into())), None);
    assert_eq!(rig.hook(&subagent.replace("agent_id", "agent_type"), HookReply::Context(CONTEXT.into())), None);
    assert_eq!(rig.calls.get(), 0);
}

#[test]
fn the_kill_switch_is_read_before_grounding_runs() {
    let rig = Rig::new();
    rig.off.set(true);
    assert_eq!(rig.hook(PROMPT, HookReply::Context(CONTEXT.into())), None);
    assert_eq!(rig.calls.get(), 0);
    rig.off.set(false);
    assert!(rig.hook(PROMPT, HookReply::Context(CONTEXT.into())).is_some());
}

#[test]
fn an_oversized_payload_prints_nothing_and_a_failed_decider_logs_and_allows() {
    let rig = Rig::new();
    let big = format!("{{\"prompt\":\"{}\"}}", "x".repeat(1048577));
    assert_eq!(rig.hook(&big, HookReply::Context(CONTEXT.into())), None);
    assert_eq!(rig.calls.get(), 0);
    assert_eq!(rig.hook("{", HookReply::Unparsable), None);
    let off = || false;
    let gate = Gate { guards_off: &off, errors: rig.dir.path().join("errors.tsv") };
    assert_eq!(decide(b"{}", &gate, |_| Err("boom".into())), None);
    let lines: Vec<_> = rig.errors().lines().map(|l| l.split('\t').skip(1).collect::<Vec<_>>().join("\t")).collect();
    assert_eq!(lines, ["prompt-grounding\tunparsable-payload", "prompt-grounding\tdecider-exception"]);
}

#[test]
fn the_audience_gate_runs_inside_the_reply_and_only_a_grounded_session_gets_context() {
    let dir = tempfile::tempdir().unwrap();
    for (name, conversation) in [("child", "s"), ("other", "elsewhere")] {
        let record = dir.path().join(name);
        std::fs::create_dir_all(&record).unwrap();
        std::fs::write(record.join("session.json"), format!("{{\n  \"name\": \"{name}\",\n  \"conversation\": \"{conversation}\"\n}}\n")).unwrap();
    }
    let grounded = |id: &str, entrypoint| crate::fleet::audience::grounded(dir.path(), None, id, Some(entrypoint));
    for (id, expected) in [("s", false), ("elsewhere", false), ("unknown", true), ("", true), ("\n", true), ("x\ny", true)] {
        assert_eq!(grounded(id, "cli"), expected, "session id {id:?} as cli");
        assert!(!grounded(id, "sdk-cli"), "session id {id:?} as sdk-cli");
    }
}
