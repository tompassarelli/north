//! The shape of a call, the key that counts recurring work: the `north` verb, or the first two
//! words of each stage after any `cd`.
use serde_json::Value;

use super::shell::stages;

pub fn shape(tool: &str, input: &Value) -> String {
    let Some(text) = (tool == "Bash").then(|| input.get("command").and_then(Value::as_str)).flatten() else {
        return tool.to_owned();
    };
    let parts: Vec<String> = stages(text)
        .into_iter()
        .filter(|stage| stage.program != "cd")
        .map(|stage| {
            let rest = stage.args.iter().map(String::as_str);
            let words: Vec<&str> = if stage.program == "north" { rest.filter(|arg| !arg.starts_with('-')).take(2).collect() } else { rest.take(1).collect() };
            std::iter::once(stage.program.as_str()).chain(words).collect::<Vec<_>>().join(" ")
        })
        .collect();
    if parts.is_empty() { tool.to_owned() } else { parts.join(" | ") }
}
