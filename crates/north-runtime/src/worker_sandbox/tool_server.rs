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
const RESULT_CHARS: usize = 4_000;
const MARKED_LINES: usize = 12;
const LINE_CHARS: usize = 300;
const REPORT_WORDS: [&str; 4] = ["Done:", "Not done:", "Split:", "Blocked:"];

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
            "description": "Finishes your lane in one call: runs `check`, the box's acceptance command, in the lane; only when it passes, commits the changed `paths` (absolute, all in one lane, the files you edited or created) with `message`, runs every safe-push gate and queues the lane, then returns the ticket without waiting for the lander. A failing check commits and queues nothing and returns its output. A change that fits in this one answer goes in `files`, each a `path` in your lane and that file's whole new `content`, written before the check, left as written when it fails and needing no place in `paths`; otherwise send your edits and this call in one message without running the check first, as this call runs after them. An accepted call ends your turn, its `Done: queued` line being your report; the lander's refusal, if any, starts your next turn, and a landing needs nothing from you.",
            "inputSchema": {"type": "object", "properties": {"check": {"type": "string"}, "message": {"type": "string"}, "paths": {"type": "array", "items": {"type": "string"}}, "files": {"type": "array", "items": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]}}}, "required": ["check", "message"]},
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
        return ["--check".to_owned(), text("check"), "--message".to_owned(), text("message"), "--files".to_owned(), "-".to_owned(), "--".to_owned()].into_iter().chain(strings(&arguments["paths"])).collect();
    }
    strings(&arguments["arguments"])
}

fn input(verb: &str, arguments: &Value) -> String {
    if verb != FINISH {
        return String::new();
    }
    arguments.get("files").filter(|files| files.is_array()).map_or_else(|| "[]".to_owned(), Value::to_string)
}

fn call(verbs: &[String], north: &Path, params: &Value) -> Result<Value, (i64, String)> {
    let verb = params["name"].as_str().filter(|name| verbs.iter().any(|verb| verb == name)).ok_or_else(|| (BAD_PARAMS, format!("no such tool: {}", params["name"])))?;
    let unstarted = |error: io::Error| (BAD_PARAMS, format!("{}: {error}", north.display()));
    let mut child = Command::new(north).arg(verb).args(words(verb, &params["arguments"])).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(unstarted)?;
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(input(verb, &params["arguments"]).as_bytes());
    }
    let output = child.wait_with_output().map_err(unstarted)?;
    let text = [output.stderr, output.stdout].map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).concat();
    Ok(json!({"content": [{"type": "text", "text": short(verb, text)}], "isError": !output.status.success()}))
}

fn short(verb: &str, text: String) -> String {
    if text.chars().count() <= RESULT_CHARS {
        return text;
    }
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let Some((verdict, before)) = lines.split_last() else { return text };
    let kept = match kept(verb, &text) {
        Ok(path) => format!("{verb}: its full output, {} lines, is in {}", lines.len(), path.display()),
        Err(error) => format!("{verb}: its full output, {} lines, could not be kept: {error}", lines.len()),
    };
    let marked = before.iter().filter(|line| line.contains('✗') || REPORT_WORDS.iter().any(|word| line.starts_with(word))).take(MARKED_LINES).map(|line| clip(line));
    marked.chain([kept, clip(verdict)]).collect::<Vec<_>>().join("\n")
}

fn clip(line: &str) -> String {
    match line.char_indices().nth(LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

fn kept(verb: &str, text: &str) -> io::Result<std::path::PathBuf> {
    let (mut file, path) = tempfile::Builder::new().prefix(&format!("north-{verb}-")).suffix(".log").tempfile()?.keep().map_err(|error| error.error)?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}
