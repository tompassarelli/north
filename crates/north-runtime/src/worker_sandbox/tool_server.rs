//! The stdio tool server a session's Claude Code starts beside it (`--mcp-config`), offering the north verbs that `harness/worker-tools.toml` names as tools. Each call runs `north VERB ARGUMENTS...` and returns what it printed, so a blocking verb comes back inline as one tool call (north#1151).

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

pub const FLAG: &str = "--tool-server";
const PROTOCOL: &str = "2025-06-18";
const NO_SUCH_METHOD: i64 = -32601;
const BAD_PARAMS: i64 = -32602;

pub fn enter(arguments: &[String], screen: super::Screen) -> Option<std::process::ExitCode> {
    match arguments.split_first() {
        Some((flag, rest)) if flag == super::FLAG => Some(super::run(rest, screen)),
        Some((flag, rest)) if flag == FLAG => Some(run(rest)),
        _ => None,
    }
}

pub fn run(verbs: &[String]) -> std::process::ExitCode {
    let north = north_base::platform::north_binary::north_binary();
    match serve(verbs, &north, io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("north tool-server: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn serve(verbs: &[String], north: &Path, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let Ok(request) = serde_json::from_str::<Value>(&line?) else { continue };
        if let Some(response) = answer(verbs, north, &request) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn answer(verbs: &[String], north: &Path, request: &Value) -> Option<Value> {
    let id = request.get("id")?;
    let params = &request["params"];
    let outcome = match request["method"].as_str() {
        Some("initialize") => Ok(json!({"protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL), "capabilities": {"tools": {}}, "serverInfo": {"name": "north", "version": env!("CARGO_PKG_VERSION")}})),
        Some("ping") => Ok(json!({})),
        Some("tools/list") => Ok(json!({"tools": verbs.iter().map(|verb| tool(verb)).collect::<Vec<_>>()})),
        Some("tools/call") => call(verbs, north, params),
        _ => Err((NO_SUCH_METHOD, "no such method".to_owned())),
    };
    Some(match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    })
}

fn tool(verb: &str) -> Value {
    json!({
        "name": verb,
        "description": format!("Runs `north {verb} ARGUMENTS...` in this session and returns what it printed."),
        "inputSchema": {"type": "object", "properties": {"arguments": {"type": "array", "items": {"type": "string"}}}},
    })
}

fn call(verbs: &[String], north: &Path, params: &Value) -> Result<Value, (i64, String)> {
    let verb = params["name"].as_str().filter(|name| verbs.iter().any(|verb| verb == name)).ok_or_else(|| (BAD_PARAMS, format!("no such tool: {}", params["name"])))?;
    let arguments: Vec<&str> = params["arguments"]["arguments"].as_array().map_or_else(Vec::new, |words| words.iter().filter_map(Value::as_str).collect());
    let output = Command::new(north).arg(verb).args(arguments).stdin(Stdio::null()).output().map_err(|error| (BAD_PARAMS, format!("{}: {error}", north.display())))?;
    let text = [output.stdout, output.stderr].map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).concat();
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": !output.status.success()}))
}
