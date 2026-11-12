use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

pub const PLAIN: [&str; 2] = ["t3", "bare"];

fn texts(content: &Value) -> Vec<&str> {
    match content {
        Value::String(text) => vec![text],
        Value::Array(blocks) => blocks.iter().flat_map(|block| block["text"].as_str().into_iter().chain(texts(&block["content"]))).collect(),
        _ => Vec::new(),
    }
}

fn text_bytes(content: &Value) -> usize {
    texts(content).iter().map(|text| text.len()).sum()
}

fn instruction_files<'a>(texts: impl Iterator<Item = &'a str>) -> BTreeSet<&'a str> {
    let named = |rest: &'a str| rest.lines().next()?.split_once(" (").map(|(path, _)| path);
    texts.flat_map(|text| text.split("Contents of ").skip(1)).filter_map(named).filter(|path| path.starts_with('/')).collect()
}

pub fn from_outside(first: &Value, repo: &str) -> Vec<String> {
    let files = first["instruction_files"].as_array().into_iter().flatten().filter_map(Value::as_str);
    files.filter(|file| !Path::new(file).starts_with(repo)).map(str::to_owned).collect()
}

fn server(name: &str) -> String {
    name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")).map_or_else(|| "native".to_owned(), |(server, _)| format!("mcp:{server}"))
}

fn by_server(tools: &[Value]) -> BTreeMap<String, Value> {
    let mut sizes: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for tool in tools {
        let entry = sizes.entry(server(tool["name"].as_str().unwrap_or_default())).or_default();
        *entry = (entry.0 + 1, entry.1 + tool.to_string().len());
    }
    sizes.into_iter().map(|(server, (count, bytes))| (server, json!({"tools": count, "json_bytes": bytes}))).collect()
}

pub fn first_request(transcript: &str) -> Result<Value, String> {
    let frames: Vec<Value> = transcript.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let of = |kind: &str| frames.iter().find(|frame| frame["type"] == kind);
    let shape = of("api-request-shape").ok_or("the transcript holds no api-request-shape record, so this Claude Code does not record its first request")?;
    let request = of("api-request").map_or(&Value::Null, |frame| &frame["params"]);
    let tools = shape["shape"]["tools"].as_array().map(Vec::as_slice).unwrap_or_default();
    let before_reply = frames.iter().take_while(|frame| frame["type"] != "assistant");
    let message: Vec<&str> = before_reply.filter(|frame| frame["type"] == "api-request-blob").flat_map(|frame| texts(&frame["message"]["content"])).collect();
    Ok(json!({
        "claude_code": shape["version"], "model": request["model"], "effort": request["output_config"]["effort"],
        "system_bytes": text_bytes(&shape["shape"]["system"]),
        "tools": tools.len(),
        "tools_deferred": tools.iter().filter(|tool| tool["defer_loading"] == true).count(),
        "tools_json_bytes": shape["shape"]["tools"].to_string().len(),
        "tools_by_server": by_server(tools),
        "first_message_bytes": message.iter().map(|text| text.len()).sum::<usize>(),
        "instruction_files": instruction_files(texts(&shape["shape"]["system"]).into_iter().chain(message.iter().copied())),
    }))
}

pub fn over_bare(row: &Value, bare: &Value) -> Value {
    let less = |key: &str| row[key].as_i64().unwrap_or(0) - bare[key].as_i64().unwrap_or(0);
    json!({"system_bytes": less("system_bytes"), "tools_json_bytes": less("tools_json_bytes"), "first_message_bytes": less("first_message_bytes")})
}
