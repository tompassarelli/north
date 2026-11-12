use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value, json};

const T3_PREFIXES: [&str; 2] = ["mcp__t3-code__", "mcp__t3_code__"];

fn tool(name: &str) -> &str {
    T3_PREFIXES.iter().find_map(|prefix| name.strip_prefix(prefix)).unwrap_or(name)
}

fn text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn main_thread(frame: &Value) -> bool {
    frame["isSidechain"] != true && frame["parent_tool_use_id"].is_null()
}

#[derive(Default)]
struct Turn {
    calls: Vec<Map<String, Value>>,
    by_id: BTreeMap<String, usize>,
    said: Vec<String>,
    last: Vec<String>,
    last_called: bool,
    response: String,
    responses: usize,
}

impl Turn {
    fn begin(&mut self, id: &str) {
        if id.is_empty() || id != self.response {
            self.response = id.to_owned();
            self.responses += 1;
            self.last.clear();
            self.last_called = false;
        }
    }

    fn say(&mut self, said: &str) {
        self.said.push(said.to_owned());
        self.last.push(said.to_owned());
    }

    fn call(&mut self, block: &Value) {
        let call = json!({"step": self.responses - 1, "tool": tool(block["name"].as_str().unwrap_or_default()), "input": block["input"], "id": block["id"]});
        self.by_id.insert(block["id"].as_str().unwrap_or_default().to_owned(), self.calls.len());
        self.calls.extend(call.as_object().cloned());
        self.last_called = true;
    }

    fn answer(&mut self, block: &Value) {
        if let Some(call) = self.by_id.get(block["tool_use_id"].as_str().unwrap_or_default()).and_then(|at| self.calls.get_mut(*at)) {
            call.insert("failed".to_owned(), json!(block["is_error"] == true));
            call.insert("output".to_owned(), json!(text(&block["content"])));
        }
    }

    fn read(&mut self, frame: &Value) {
        let blocks = frame["message"]["content"].as_array().map(Vec::as_slice).unwrap_or_default();
        let of = |kind: &'static str| blocks.iter().filter(move |block| block["type"] == kind);
        if frame["type"] == "assistant" {
            self.begin(frame["message"]["id"].as_str().unwrap_or_default());
            of("text").filter_map(|block| block["text"].as_str()).for_each(|said| self.say(said));
            of("tool_use").for_each(|block| self.call(block));
        } else if frame["type"] == "user" {
            of("tool_result").for_each(|block| self.answer(block));
        }
    }
}

fn turn(transcript: &str, planted: &HashSet<String>) -> Turn {
    let mut turn = Turn::default();
    transcript.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(main_thread).filter(|frame| !planted.contains(frame["uuid"].as_str().unwrap_or_default())).for_each(|frame| turn.read(&frame));
    turn
}

pub fn row(case: &Value, transcript: &str, cwd: &str, planted: &HashSet<String>) -> Value {
    let turn = turn(transcript, planted);
    let ended = turn.responses > 0 && !turn.last_called;
    json!({
        "id": case["id"], "kind": case["kind"], "pattern": case["pattern"], "role": case["setup"]["role"],
        "calls": turn.calls, "text": turn.said.join("\n\n"),
        "stopReason": if ended { "end_turn" } else { "max_steps" },
        "finalText": if ended { turn.last.join("\n\n") } else { String::new() },
        "cwd": cwd, "linked": [], "pass": true, "usage": {}, "shellUsage": {},
    })
}

pub fn graded(table: &str, row: &Value, stamp: Value) -> Result<Value, String> {
    let id = row["id"].as_str().unwrap_or_default();
    let cells: Vec<&str> = table.lines().map(|line| line.split('|').map(str::trim).collect::<Vec<_>>()).find(|cells| cells.len() >= 4 && cells[1] == id).ok_or_else(|| format!("T3 Code's regrade printed no row for {id}:\n{table}"))?;
    let now = cells[3].split_whitespace().next().unwrap_or_default();
    let pass = match now {
        "1/1" => true,
        "0/1" => false,
        other => return Err(format!("T3 Code's regrade gave {other} for one sample of {id}")),
    };
    let failed = table.lines().find_map(|line| line.strip_prefix(&format!("{id}#1 ")).and_then(|rest| rest.split_once(": ")).map(|(_, checks)| checks.to_owned()));
    let mut out = json!({"case": id, "kind": row["kind"], "pass": pass, "cut_short": cells[3].contains("cut short"), "failed_checks": failed, "calls": row["calls"].as_array().map_or(0, Vec::len), "stop_reason": row["stopReason"], "final_text_bytes": row["finalText"].as_str().map_or(0, str::len)});
    if let (Some(out), Some(stamp)) = (out.as_object_mut(), stamp.as_object()) {
        out.extend(stamp.clone());
    }
    Ok(out)
}
