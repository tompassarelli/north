use serde_json::{Value, json};

pub(crate) const INPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct HookInput {
    pub data: Value,
    pub tool_input: Value,
    pub raw: Vec<u8>,
    command: Option<String>,
}

impl HookInput {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if std::str::from_utf8(bytes).ok()?.chars().count() as u64 > INPUT_LIMIT {
            return None;
        }
        let data = read_json(bytes)?;
        data.as_object()?;
        let patch = data["tool_name"]
            .as_str()
            .is_some_and(|tool| tool.ends_with("apply_patch"));
        let tool_input = match data.get("tool_input") {
            Some(Value::String(text)) => match read_json(text.as_bytes()) {
                Some(value) => value,
                None if patch => Value::String(text.clone()),
                None => return None,
            },
            Some(value) => value.clone(),
            None => json!({}),
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

    pub fn runs_in_background(&self) -> bool {
        self.tool_input["run_in_background"].as_bool() == Some(true) || self.is_monitor()
    }

    // Claude Code's hook event format hands a Monitor call to a hook matched on Bash as tool Bash without timeout_ms, so the matcher must name Monitor.
    fn is_monitor(&self) -> bool {
        self.tool_name() == "Monitor"
    }

    pub fn is_shell(&self) -> bool {
        crate::guard::shell_call::is_shell_tool(self.tool_name())
    }

    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
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
    End(String),
    /// A row for the run ledger; the guard lets the command through, and the hook's caller writes the row.
    Record(Evidence),
}

#[derive(Debug, PartialEq)]
pub enum Evidence {
    NativeSolo { session: String, reason: String, command: String },
}

/// The JSON document in `bytes`, with each unpaired UTF-16 surrogate escape read as U+FFFD, since a JavaScript harness writes a lone surrogate as `\udXXX` and serde_json refuses it.
pub fn read_json(bytes: &[u8]) -> Option<Value> {
    let unit = |at: usize| bytes.get(at..at + 6).filter(|escape| escape.starts_with(b"\\u")).and_then(|escape| u16::from_str_radix(std::str::from_utf8(&escape[2..]).ok()?, 16).ok());
    let mut paired = Vec::new();
    let (mut at, mut copied) = (0, 0);
    while at + 1 < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        match unit(at) {
            Some(0xd800..=0xdbff) if matches!(unit(at + 6), Some(0xdc00..=0xdfff)) => at += 12,
            Some(0xd800..=0xdfff) => {
                paired.extend_from_slice(&bytes[copied..at]);
                paired.extend_from_slice(b"\\ufffd");
                at += 6;
                copied = at;
            }
            _ => at += 2,
        }
    }
    if paired.is_empty() {
        return serde_json::from_slice(bytes).ok();
    }
    paired.extend_from_slice(&bytes[copied..]);
    serde_json::from_slice(&paired).ok()
}
