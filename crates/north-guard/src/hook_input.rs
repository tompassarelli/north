use crate::platform::pyjson::{self, Value};

pub(crate) const INPUT_LIMIT: u64 = 1024 * 1024;

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
    End(String),
}
