use serde_json::Value;

const SHELL_TOOLS: [&str; 6] = ["Bash", "Monitor", "exec_command", "shell", "shell_command", "local_shell"];

pub fn is_shell_tool(name: &str) -> bool {
    SHELL_TOOLS.contains(&name)
}

pub fn command_text(tool_name: &str, tool_input: &Value) -> Option<String> {
    if !is_shell_tool(tool_name.rsplit('.').next()?) {
        return None;
    }
    match &tool_input["command"] {
        Value::String(text) => Some(text.clone()),
        Value::Array(words) => {
            let words = words.iter().map(|w| w.as_str()).collect::<Option<Vec<_>>>()?;
            let text = match words.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => {
                    (*script).to_owned()
                }
                _ => words.join(" "),
            };
            Some(text)
        }
        _ => tool_input["cmd"].as_str().map(str::to_owned),
    }
}
