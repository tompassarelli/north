use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::path::PathBuf;

const INPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct HookInput {
    pub data: Value,
    pub tool_input: Value,
}

impl HookInput {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() as u64 > INPUT_LIMIT {
            return None;
        }
        let data: Value = serde_json::from_slice(bytes).ok()?;
        data.as_object()?;
        let tool_input = match data.get("tool_input") {
            Some(Value::String(text)) => serde_json::from_str(text).ok()?,
            Some(value) => value.clone(),
            None => json!({}),
        };
        if !tool_input.is_object() && !tool_input.is_null() {
            return None;
        }
        Some(Self { data, tool_input })
    }

    pub fn tool_name(&self) -> &str {
        self.data["tool_name"]
            .as_str()
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("")
    }

    pub fn command(&self) -> Option<&str> {
        self.tool_input[if self.tool_name() == "Bash" {
            "command"
        } else {
            "cmd"
        }]
        .as_str()
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
    Ask(String),
    Context(String),
}

pub type Guard = fn(&HookInput) -> Verdict;

pub fn evaluate(input: &HookInput, guards: &[Guard], event: &str) -> Option<Value> {
    let mut context = Vec::new();
    let mut ask = None;
    for guard in guards {
        match guard(input) {
            Verdict::Allow => {}
            Verdict::Deny(reason) if event == "Stop" => {
                return Some(json!({"decision": "block", "reason": reason}));
            }
            Verdict::Deny(reason) => {
                return Some(json!({"hookSpecificOutput": {
                    "hookEventName": event, "permissionDecision": "deny", "permissionDecisionReason": reason
                }}));
            }
            Verdict::Ask(reason) => {
                if ask.is_none() {
                    ask = Some(reason);
                }
            }
            Verdict::Context(text) => context.push(text),
        }
    }
    let mut output = json!({"hookEventName": event});
    if let Some(reason) = ask {
        output["permissionDecision"] = json!("ask");
        output["permissionDecisionReason"] = json!(reason);
    }
    if !context.is_empty() {
        output["additionalContext"] = json!(context.join("\n"));
    }
    (output.as_object()?.len() > 1).then(|| json!({"hookSpecificOutput": output}))
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

fn digest(value: &Value) -> bool {
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
    if list.is_file() {
        if let Ok(contents) = std::fs::read(&list) {
            let wanted = format!("hook {id}");
            return !contents
                .split(|b| *b == b'\n')
                .any(|line| line.iter().copied().filter(|b| *b != 0).eq(wanted.bytes()));
        }
    }
    let Some(data) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
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

pub fn authoring_guards_off(id: &str) -> bool {
    guards_off(
        &nonempty_env("NORTH_HOOK_ID").unwrap_or_else(|| id.to_owned()),
        &nonempty_env,
    )
}

fn process(
    reader: impl Read,
    writer: &mut impl Write,
    guards: &[Guard],
    event: &str,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    if reader
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(());
    }
    if let Some(input) = HookInput::parse(&bytes) {
        if let Some(output) = evaluate(&input, guards, event) {
            serde_json::to_writer(&mut *writer, &output)?;
            writer.write_all(b"\n")?;
        }
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [kind] = arguments else {
        return Err(
            "Usage: north hook pre-bash|pre-edit|post-tool|session-start|stop|pre-agent".into(),
        );
    };
    let event = match kind.as_str() {
        "pre-bash" | "pre-edit" | "pre-agent" => "PreToolUse",
        "post-tool" => "PostToolUse",
        "session-start" => "SessionStart",
        "stop" => "Stop",
        _ => return Err(format!("unknown hook '{kind}'")),
    };
    let guards: &[Guard] = &[];
    let _ = process(io::stdin().lock(), &mut io::stdout().lock(), guards, event);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny(input: &HookInput) -> Verdict {
        assert_eq!(input.command(), Some("echo ready"));
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
            &[|_| Verdict::Context("at limit".into())],
            "Stop",
        )
        .unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn ask_and_context_survive_allow_but_deny_wins() {
        let input = HookInput::parse(b"{}").unwrap();
        let guards: &[Guard] = &[
            |_| Verdict::Context("first".into()),
            |_| Verdict::Allow,
            |_| Verdict::Ask("confirm".into()),
            |_| Verdict::Context("second".into()),
        ];
        assert_eq!(
            evaluate(&input, guards, "PreToolUse"),
            Some(
                json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"ask", "permissionDecisionReason":"confirm", "additionalContext":"first\nsecond"}})
            )
        );
        assert_eq!(evaluate(&input, &[], "Stop"), None);
        assert_eq!(
            evaluate(
                &input,
                &[
                    |_| Verdict::Ask("ask".into()),
                    |_| Verdict::Deny("deny".into())
                ],
                "PreToolUse"
            )
            .unwrap()["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            evaluate(
                &input,
                &[|_| Verdict::Deny("stop\nverbatim".into())],
                "Stop"
            ),
            Some(json!({"decision":"block", "reason":"stop\nverbatim"}))
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
