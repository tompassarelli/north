use crate::platform::pyjson::{self, Value};
use std::io::{self, Read, Write};
use std::path::PathBuf;

const INPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct HookInput {
    pub data: Value,
    pub tool_input: Value,
    pub raw: Vec<u8>,
    command: Option<pyjson::PyString>,
}

impl HookInput {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if std::str::from_utf8(bytes).ok()?.chars().count() as u64 > INPUT_LIMIT {
            return None;
        }
        let data = pyjson::loads(bytes).ok()?;
        data.as_object()?;
        let patch = data["tool_name"]
            .as_str()
            .is_some_and(|tool| tool.ends_with("apply_patch"));
        let tool_input = match data.get("tool_input") {
            Some(Value::String(text)) => match pyjson::loads_string(text) {
                Ok(value) => value,
                Err(_) if patch => Value::String(text.clone()),
                Err(_) => return None,
            },
            Some(value) => value.clone(),
            None => Value::object([]),
        };
        if !patch && !tool_input.is_object() && !tool_input.is_null() {
            return None;
        }
        let command = crate::guard::shell_call::command_text(data["tool_name"].as_str().unwrap_or(""), &tool_input);
        Some(Self {
            data,
            tool_input,
            raw: bytes.to_vec(),
            command,
        })
    }

    pub fn tool_name(&self) -> &str {
        self.data["tool_name"]
            .as_str()
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("")
    }

    pub fn is_shell(&self) -> bool {
        crate::guard::shell_call::is_shell_tool(self.tool_name())
    }

    pub fn command(&self) -> Option<&pyjson::PyString> {
        self.command.as_ref()
    }

    pub fn cwd(&self) -> Option<&str> {
        self.tool_input["workdir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| self.data["cwd"].as_str())
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allow,
    Deny(String),
    Rewrite(Value, String),
    Advise(String),
    Approve,
}

pub type Guard = fn(&HookInput) -> Verdict;

#[derive(Debug, Clone, Copy, PartialEq)]
enum HookKind {
    PreBash,
    PreEdit,
    PreAgent,
    PostTool,
    PermissionRequest,
    Stop,
    SubagentStop,
    SessionStart,
    UserPromptSubmit,
    SessionKill,
}

impl HookKind {
    fn explicit(kind: &str) -> Option<Self> {
        Some(match kind {
            "pre-bash" => Self::PreBash,
            "pre-edit" => Self::PreEdit,
            "pre-agent" => Self::PreAgent,
            "post-tool" => Self::PostTool,
            "permission-request" => Self::PermissionRequest,
            "stop" => Self::Stop,
            "subagent-stop" => Self::SubagentStop,
            "session-start" => Self::SessionStart,
            "user-prompt-submit" => Self::UserPromptSubmit,
            "session-kill" => Self::SessionKill,
            _ => return None,
        })
    }

    fn from_input(input: &HookInput) -> Option<Self> {
        Some(match input.data["hook_event_name"].as_str()? {
            "PreToolUse" => match input.tool_name() {
                name if crate::guard::shell_call::is_shell_tool(name) => Self::PreBash,
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => Self::PreEdit,
                "Agent" | "spawn_agent" => Self::PreAgent,
                _ => return None,
            },
            "PostToolUse" => Self::PostTool,
            "PermissionRequest" => Self::PermissionRequest,
            "Stop" => Self::Stop,
            "SubagentStop" => Self::SubagentStop,
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::UserPromptSubmit,
            _ => return None,
        })
    }

    fn event(self) -> &'static str {
        match self {
            Self::PreBash | Self::PreEdit | Self::PreAgent | Self::SessionKill => "PreToolUse",
            Self::PostTool => "PostToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
        }
    }

    fn guards(self) -> &'static [Guard] {
        match self {
            Self::PreBash => &[
                crate::guard::tripwire::check,
                crate::guard::corpus_scan::evaluate,
                crate::guard::resource_safe_search::evaluate,
                crate::guard::modern_search::evaluate,
                crate::guard::launch_critical::evaluate,
                crate::guard::desktop_action::decide,
                crate::guard::blind_stage::evaluate,
                crate::guard::reviewer_cli::evaluate,
                crate::guard::git_stash::evaluate,
                crate::guard::native_evidence::evaluate,
                crate::guard::issue_labels::evaluate,
                crate::guard::sops_decrypt::evaluate,
                crate::guard::public_text::evaluate,
                crate::guard::frozen_topics::evaluate,
                crate::guard::poll_loop::evaluate,
                crate::guard::heavy_command::evaluate,
                crate::guard::session_kill::evaluate,
                crate::guard::clipboard::evaluate,
                crate::guard::worker_wait::evaluate,
                crate::guard::native_launch::evaluate,
                crate::guard::native_item::evaluate,
                crate::guard::generated_policy::evaluate,
                crate::guard::model_identity::evaluate,
            ],
            Self::PreEdit => &[
                crate::guard::launch_critical::evaluate,
                crate::guard::handoff_record::evaluate,
                crate::guard::generated_policy::evaluate,
                crate::guard::model_identity::evaluate,
            ],
            Self::PreAgent => &[crate::guard::worker_tier::evaluate],
            Self::PostTool => &[crate::guard::worker_handoff::evaluate, crate::guard::code_health_edit::evaluate],
            Self::PermissionRequest => &[crate::guard::tripwire::check],
            Self::Stop => &[],
            Self::SubagentStop => &[crate::guard::subagent_teardown::evaluate],
            Self::SessionStart => &[crate::guard::unlanded_work::evaluate],
            Self::UserPromptSubmit => &[],
            Self::SessionKill => &[crate::guard::session_kill::evaluate],
        }
    }
}

fn permission_decision(behavior: &str, message: Option<String>) -> Value {
    let mut decision = Value::object([("behavior", behavior.into())]);
    if let Some(message) = message {
        decision["message"] = message.into();
    }
    Value::object([(
        "hookSpecificOutput",
        Value::object([
            ("hookEventName", "PermissionRequest".into()),
            ("decision", decision),
        ]),
    )])
}

pub fn evaluate(input: &HookInput, guards: &[Guard], event: &str) -> Option<Value> {
    let mut context = Vec::new();
    let mut updated_input = None;
    let mut rewritten = None;
    for guard in guards {
        match guard(rewritten.as_ref().unwrap_or(input)) {
            Verdict::Allow => {}
            Verdict::Approve if event == "PermissionRequest" => {
                return Some(permission_decision("allow", None));
            }
            Verdict::Approve => {}
            Verdict::Deny(reason) if event == "PermissionRequest" => {
                return Some(permission_decision("deny", Some(reason)));
            }
            Verdict::Deny(reason) if matches!(event, "Stop" | "SubagentStop") => {
                return Some(Value::object([
                    ("decision", "block".into()),
                    ("reason", reason.into()),
                ]));
            }
            Verdict::Deny(reason) => {
                return Some(Value::object([(
                    "hookSpecificOutput",
                    Value::object([
                        ("hookEventName", event.into()),
                        ("permissionDecision", "deny".into()),
                        ("permissionDecisionReason", reason.into()),
                    ]),
                )]));
            }
            Verdict::Advise(text) => context.push(text),
            Verdict::Rewrite(updated, text) => {
                let mut data = input.data.clone();
                data["tool_input"] = updated.clone();
                rewritten = HookInput::parse(pyjson::dumps(&data).as_bytes());
                updated_input = Some(updated);
                context.push(text);
            }
        }
    }
    let mut output = Value::object([("hookEventName", event.into())]);
    if let Some(updated) = updated_input {
        output["permissionDecision"] = "allow".into();
        output["updatedInput"] = updated;
    }
    if !context.is_empty() {
        output["additionalContext"] = context.join("\n").into();
    }
    (output.as_object()?.len() > 1).then(|| Value::object([("hookSpecificOutput", output)]))
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn activation_paths(get: &impl Fn(&str) -> Option<String>) -> (PathBuf, PathBuf) {
    let root = get("NORTH_AGENT_STATE_ROOT").unwrap_or_else(|| {
        format!(
            "{}/.local/state/north/agents",
            get("HOME").unwrap_or_default()
        )
    });
    let activation = get("NORTH_AGENT_ACTIVATION");
    let active = get("NORTH_AGENT_ACTIVE");
    let list = active
        .clone()
        .or_else(|| {
            activation
                .as_ref()
                .map(|s| format!("{}.active", s.strip_suffix(".json").unwrap_or(s)))
        })
        .unwrap_or_else(|| format!("{root}/current/activation.active"));
    let json = activation
        .or_else(|| active.map(|s| format!("{}.json", s.strip_suffix(".active").unwrap_or(&s))))
        .unwrap_or_else(|| format!("{root}/current/activation.json"));
    (list.into(), json.into())
}

fn digest(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn guards_off(id: &str, get: &impl Fn(&str) -> Option<String>) -> bool {
    if let Some(value) = get("AGENT_NO_AUTHORING_HOOKS").filter(|s| !s.is_empty()) {
        return !matches!(value.as_str(), "0" | "false");
    }
    let id = id.strip_suffix(".sh").unwrap_or(id);
    let id = id.strip_suffix(".js").unwrap_or(id);
    let (list, path) = activation_paths(get);
    if list.is_file()
        && let Ok(contents) = std::fs::read(&list) {
            let wanted = format!("hook {id}");
            return !contents
                .split(|b| *b == b'\n')
                .any(|line| line.iter().copied().filter(|b| *b != 0).eq(wanted.bytes()));
        }
    let Some(data) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return true;
    };
    if data["schema"] != "north.agent-activation/v1"
        || !digest(&data["catalogDigest"])
        || !digest(&data["generationId"])
    {
        return true;
    }
    let Some(units) = data["units"].as_array() else {
        return true;
    };
    let matches: Vec<_> = units
        .iter()
        .filter(|u| u["kind"] == "hook" && u["id"] == id)
        .collect();
    matches.len() != 1 || matches[0]["permission"] != "on" || matches[0]["active"] != true
}

#[cfg(test)]
thread_local! {
    static TEST_GUARDS_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn authoring_guards_off(id: &str) -> bool {
    #[cfg(test)]
    if TEST_GUARDS_OFF.with(|off| off.get()) {
        return true;
    }
    guards_off_in(id, &nonempty_env)
}

pub fn guards_off_in(id: &str, get: &impl Fn(&str) -> Option<String>) -> bool {
    guards_off(&get("NORTH_HOOK_ID").unwrap_or_else(|| id.to_owned()), get)
}

fn process(
    reader: impl Read,
    writer: &mut impl Write,
    guards: &[Guard],
    event: &str,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    if reader
        .take(INPUT_LIMIT * 4 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(());
    }
    if let Some(input) = HookInput::parse(&bytes) {
        let (guards, event) = if event.is_empty() {
            let Some(kind) = HookKind::from_input(&input) else {
                return Ok(());
            };
            (kind.guards(), kind.event())
        } else {
            (guards, event)
        };
        if let Some(output) = evaluate(&input, guards, event) {
            writer.write_all(pyjson::dumps(&output).as_bytes())?;
            writer.write_all(b"\n")?;
        }
    } else if bytes.len() as u64 <= INPUT_LIMIT && (event == "PreToolUse" || event.is_empty()) {
        crate::guard::launch_critical::record_invalid_input(&bytes);
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let kind = match arguments {
        [] => None,
        [kind] => Some(HookKind::explicit(kind).ok_or_else(|| format!("unknown hook '{kind}'"))?),
        _ => return Err("Usage: north hook [pre-bash|pre-edit|post-tool|session-start|stop|pre-agent|subagent-stop|user-prompt-submit|permission-request|session-kill]".into()),
    };
    let _ = process(
        io::stdin().lock(),
        &mut io::stdout().lock(),
        kind.map_or(&[], HookKind::guards),
        kind.map_or("", HookKind::event),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn serde_output(output: Option<pyjson::Value>) -> Option<Value> {
        output.map(|value| serde_json::from_str(&pyjson::dumps(&value)).unwrap())
    }

    #[test]
    fn provider_events_select_matching_guard_list() {
        TEST_GUARDS_OFF.with(|off| off.set(true));
        for (event, tools, explicit, expected) in [
            (
                "PreToolUse",
                vec!["Bash", "exec_command", "functions.exec_command"],
                "pre-bash",
                HookKind::PreBash,
            ),
            (
                "PreToolUse",
                vec![
                    "Edit",
                    "Write",
                    "MultiEdit",
                    "NotebookEdit",
                    "apply_patch",
                    "functions.apply_patch",
                ],
                "pre-edit",
                HookKind::PreEdit,
            ),
            (
                "PreToolUse",
                vec!["Agent", "spawn_agent", "functions.spawn_agent"],
                "pre-agent",
                HookKind::PreAgent,
            ),
            (
                "PostToolUse",
                vec!["Bash", "functions.exec_command"],
                "post-tool",
                HookKind::PostTool,
            ),
            (
                "PermissionRequest",
                vec!["Bash"],
                "permission-request",
                HookKind::PermissionRequest,
            ),
            ("Stop", vec![""], "stop", HookKind::Stop),
            (
                "SubagentStop",
                vec![""],
                "subagent-stop",
                HookKind::SubagentStop,
            ),
            (
                "SessionStart",
                vec![""],
                "session-start",
                HookKind::SessionStart,
            ),
            (
                "UserPromptSubmit",
                vec![""],
                "user-prompt-submit",
                HookKind::UserPromptSubmit,
            ),
        ] {
            assert_eq!(HookKind::explicit(explicit), Some(expected));
            for tool in tools {
                for tool_input in [json!({}), json!("{}")] {
                    let bytes = serde_json::to_vec(&json!({"hook_event_name":event, "tool_name":tool, "tool_input":tool_input})).unwrap();
                    let input = HookInput::parse(&bytes).unwrap();
                    assert_eq!(
                        HookKind::from_input(&input),
                        Some(expected),
                        "{event} {tool}"
                    );
                    assert_eq!(expected.event(), event);
                    let mut output = Vec::new();
                    process(bytes.as_slice(), &mut output, &[], "").unwrap();
                    assert!(output.is_empty());
                }
            }
        }
        for payload in [
            json!({}),
            json!({"hook_event_name":"unknown"}),
            json!({"hook_event_name":"PreToolUse", "tool_name":"Read"}),
        ] {
            let bytes = serde_json::to_vec(&payload).unwrap();
            assert_eq!(
                HookKind::from_input(&HookInput::parse(&bytes).unwrap()),
                None
            );
            let mut output = Vec::new();
            process(
                bytes.as_slice(),
                &mut output,
                &[|_| panic!("unknown event reached guard")],
                "",
            )
            .unwrap();
            assert!(output.is_empty());
        }
    }

    fn deny(input: &HookInput) -> Verdict {
        assert_eq!(
            input.command().and_then(|text| text.as_str()),
            Some("echo ready")
        );
        assert_eq!(input.cwd(), Some("/tmp"));
        Verdict::Deny("fixture denial\nverbatim \"reason\"".into())
    }

    #[test]
    fn fixture_denial_preserves_both_provider_inputs_and_first_message() {
        for payload in [
            json!({"tool_name":"Bash", "cwd":"/tmp", "tool_input":{"command":"echo ready"}}),
            json!({"tool_name":"functions.exec_command", "cwd":"/other", "tool_input":"{\"cmd\":\"echo ready\",\"workdir\":\"/tmp\"}"}),
        ] {
            let mut out = Vec::new();
            process(
                serde_json::to_vec(&payload).unwrap().as_slice(),
                &mut out,
                &[deny, |_| panic!("ran after deny")],
                "PreToolUse",
            )
            .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&out).unwrap(),
                json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"deny", "permissionDecisionReason":"fixture denial\nverbatim \"reason\""}})
            );
        }
        for bytes in [
            br#"{"tool_name":"Bash","cwd":"/tmp","tool_input":{"command":"wtype hi","opaque":"\ud800"}}"#.as_slice(),
            br#"{"tool_name":"functions.exec_command","cwd":"/tmp","tool_input":"{\"cmd\":\"wtype hi\",\"opaque\":\"\\ud800\"}"}"#.as_slice(),
            br#"{"tool_name":"Bash","cwd":"/tmp","tool_input":{"command":"wtype hi\ud800","opaque":"\ud800"}}"#.as_slice(),
        ] {
            let mut output = Vec::new();
            process(bytes, &mut output, &[|input| {
                assert!(input.command().unwrap().codepoints.starts_with(&"wtype hi".chars().map(u32::from).collect::<Vec<_>>()));
                Verdict::Deny("No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.".into())
            }, |_| panic!("ran after deny")], "PreToolUse").unwrap();
            assert_eq!(output, b"{\"hookSpecificOutput\": {\"hookEventName\": \"PreToolUse\", \"permissionDecision\": \"deny\", \"permissionDecisionReason\": \"No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.\"}}\n");
            eprintln!("desktop-action opaque surrogate: fixture deny matches Python's 260-byte denial");
        }
    }

    #[test]
    fn python_rewrite_output_preserves_both_provider_inputs() {
        for bytes in [
            br#"{"tool_name":"Bash","tool_input":{"command":"echo ready","opaque":"\ud800"}}"#.as_slice(),
            br#"{"tool_name":"functions.exec_command","tool_input":"{\"cmd\":\"echo ready\",\"opaque\":\"\\ud800\"}"}"#.as_slice(),
        ] {
            let mut output = Vec::new();
            process(bytes, &mut output, &[|input| {
                let mut updated = input.tool_input.clone();
                let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
                updated[field] = "wrapped".into();
                Verdict::Rewrite(updated, "fixture context".into())
            }], "PreToolUse").unwrap();
            let field = if bytes.starts_with(br#"{"tool_name":"Bash""#) { "command" } else { "cmd" };
            assert_eq!(String::from_utf8(output).unwrap(), format!("{{\"hookSpecificOutput\": {{\"hookEventName\": \"PreToolUse\", \"permissionDecision\": \"allow\", \"updatedInput\": {{\"{field}\": \"wrapped\", \"opaque\": \"\\ud800\"}}, \"additionalContext\": \"fixture context\"}}}}\n"));
        }
    }

    #[test]
    fn python_character_limit_accepts_multibyte_payloads() {
        for character in ['é', '🦀'] {
            for count in [600_000, INPUT_LIMIT as usize - 8, INPUT_LIMIT as usize - 7] {
                let bytes =
                    format!("{{\"x\":\"{}\"}}", character.to_string().repeat(count)).into_bytes();
                let accepted = count + 8 <= INPUT_LIMIT as usize;
                assert_eq!(HookInput::parse(&bytes).is_some(), accepted);
                let mut output = Vec::new();
                process(
                    bytes.as_slice(),
                    &mut output,
                    &[|_| Verdict::Deny("accepted".into())],
                    "Stop",
                )
                .unwrap();
                assert_eq!(!output.is_empty(), accepted);
            }
        }
    }

    #[test]
    fn malformed_and_oversized_payloads_fail_open() {
        let mut oversized = b"{}".to_vec();
        oversized.resize(INPUT_LIMIT as usize + 1, b' ');
        for bytes in [
            b"{bad".to_vec(),
            b"null".to_vec(),
            b"[]".to_vec(),
            b"{\"tool_input\":\"bad\"}".to_vec(),
            oversized,
        ] {
            let mut output = Vec::new();
            process(
                bytes.as_slice(),
                &mut output,
                &[|_| panic!("invalid payload reached guard")],
                "PreToolUse",
            )
            .unwrap();
            assert!(output.is_empty());
        }
        let mut bytes = b"{}".to_vec();
        bytes.resize(INPUT_LIMIT as usize, b' ');
        let mut output = Vec::new();
        process(
            bytes.as_slice(),
            &mut output,
            &[|_| Verdict::Deny("at limit".into())],
            "Stop",
        )
        .unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn stop_denial_blocks_verbatim_and_no_guards_say_nothing() {
        let input = HookInput::parse(b"{}").unwrap();
        assert_eq!(serde_output(evaluate(&input, &[], "Stop")), None);
        assert_eq!(
            serde_output(evaluate(
                &input,
                &[|_| Verdict::Deny("stop\nverbatim".into())],
                "Stop"
            )),
            Some(json!({"decision":"block", "reason":"stop\nverbatim"}))
        );
    }

    #[test]
    fn subagent_stop_denial_blocks_at_top_level() {
        let input = HookInput::parse(b"{}").unwrap();
        assert_eq!(
            serde_output(evaluate(
                &input,
                &[|_| Verdict::Deny("still running".into())],
                "SubagentStop"
            )),
            Some(json!({"decision":"block", "reason":"still running"}))
        );
    }

    #[test]
    fn killswitch_matches_override_list_and_activation_json() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("activation.active");
        let path = dir.path().join("activation.json");
        let mut env =
            std::collections::BTreeMap::from([("NORTH_AGENT_ACTIVE", list.display().to_string())]);
        let off = |env: &std::collections::BTreeMap<&str, String>| {
            guards_off("fixture.sh", &|name| env.get(name).cloned())
        };
        assert!(off(&env));
        for (value, expected) in [
            ("0", false),
            ("false", false),
            ("1", true),
            ("true", true),
            ("FALSE", true),
            (" ", true),
        ] {
            env.insert("AGENT_NO_AUTHORING_HOOKS", value.into());
            assert_eq!(off(&env), expected);
        }
        env.remove("AGENT_NO_AUTHORING_HOOKS");
        std::fs::write(&list, "hook other\nhook fixture").unwrap();
        assert!(!off(&env));
        std::fs::write(&list, "hook fixture\r\n").unwrap();
        assert!(off(&env));
        std::fs::remove_file(&list).unwrap();
        let mut activation = json!({"schema":"north.agent-activation/v1", "catalogDigest":format!("sha256:{}", "a".repeat(64)), "generationId":format!("sha256:{}", "0".repeat(64)), "units":[{"kind":"hook", "id":"fixture", "permission":"on", "active":true}]});
        std::fs::write(&path, activation.to_string()).unwrap();
        assert!(!off(&env));
        activation["units"][0]["active"] = json!(false);
        std::fs::write(&path, activation.to_string()).unwrap();
        assert!(off(&env));
        activation["units"][0]["active"] = json!(true);
        let duplicate = activation["units"][0].clone();
        activation["units"].as_array_mut().unwrap().push(duplicate);
        std::fs::write(&path, activation.to_string()).unwrap();
        assert!(off(&env));
        std::fs::write(&path, "bad").unwrap();
        assert!(off(&env));
    }
}

#[cfg(test)]
#[path = "hook_rewrite_tests.rs"]
mod rewrite_tests;
