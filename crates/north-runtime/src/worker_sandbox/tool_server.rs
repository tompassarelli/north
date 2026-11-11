//! The stdio tool server a session's Claude Code starts beside it (`--mcp-config`), offering the north verbs that `harness/worker-tools.toml` names as tools. Each call runs `north VERB ARGUMENTS...` and returns what it printed, so a blocking verb comes back inline as one tool call (north#1151).

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

pub const FLAG: &str = "--tool-server";
const PROTOCOL: &str = "2025-06-18";
const NO_SUCH_METHOD: i64 = -32601;
const BAD_PARAMS: i64 = -32602;
const HAND_IN: &str = "hand-in";
const FINISH: &str = "finish";

pub fn enter(arguments: &[String], screen: super::Screen) -> Option<std::process::ExitCode> {
    match arguments.split_first() {
        Some((flag, rest)) if flag == super::FLAG => Some(super::run(rest, screen)),
        Some((flag, rest)) if flag == FLAG => Some(run(rest)),
        _ => None,
    }
}

pub fn run(verbs: &[String]) -> std::process::ExitCode {
    let north = north_base::platform::north_binary::own_binary();
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
    if verb == HAND_IN {
        return json!({
            "name": verb,
            "description": "Hands in your finished lane in one call: commits the changed `paths` (absolute, all in one lane) with `message`, or queues the lane as it stands when they are already committed, then runs every safe-push gate, queues the lane and waits for the lander. Returns `Done: landed COMMIT on main`, or why it did not land.",
            "inputSchema": {"type": "object", "properties": {"message": {"type": "string"}, "paths": {"type": "array", "items": {"type": "string"}}}, "required": ["message", "paths"]},
        });
    }
    if verb == FINISH {
        return json!({
            "name": verb,
            "description": "Finishes your lane in one call: runs `check`, the box's acceptance command, in the lane; only when it passes, commits the changed `paths` (absolute, all in one lane, the files you edited or created) with `message`, runs every safe-push gate and queues the lane, then returns the ticket without waiting for the lander. A failing check commits and queues nothing and returns its output. The lander's refusal, if any, starts your next turn; a landing needs nothing from you.",
            "inputSchema": {"type": "object", "properties": {"check": {"type": "string"}, "message": {"type": "string"}, "paths": {"type": "array", "items": {"type": "string"}}}, "required": ["check", "message", "paths"]},
        });
    }
    json!({
        "name": verb,
        "description": format!("Runs `north {verb} ARGUMENTS...` in this session and returns what it printed."),
        "inputSchema": {"type": "object", "properties": {"arguments": {"type": "array", "items": {"type": "string"}}}},
    })
}

fn words(verb: &str, arguments: &Value) -> Vec<String> {
    let strings = |value: &Value| value.as_array().map_or_else(Vec::new, |words| words.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    if verb == HAND_IN {
        let message = arguments["message"].as_str().unwrap_or_default().to_owned();
        return ["--message".to_owned(), message, "--".to_owned()].into_iter().chain(strings(&arguments["paths"])).collect();
    }
    if verb == FINISH {
        let text = |key: &str| arguments[key].as_str().unwrap_or_default().to_owned();
        return ["--check".to_owned(), text("check"), "--message".to_owned(), text("message"), "--".to_owned()].into_iter().chain(strings(&arguments["paths"])).collect();
    }
    strings(&arguments["arguments"])
}

fn call(verbs: &[String], north: &Path, params: &Value) -> Result<Value, (i64, String)> {
    let verb = params["name"].as_str().filter(|name| verbs.iter().any(|verb| verb == name)).ok_or_else(|| (BAD_PARAMS, format!("no such tool: {}", params["name"])))?;
    let output = Command::new(north).arg(verb).args(words(verb, &params["arguments"])).stdin(Stdio::null()).output().map_err(|error| (BAD_PARAMS, format!("{}: {error}", north.display())))?;
    let text = [output.stderr, output.stdout].map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).concat();
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": !output.status.success()}))
}
