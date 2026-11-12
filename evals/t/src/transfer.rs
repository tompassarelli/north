use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};

pub const SESSION: &str = "eval-t-transfer";
const HISTORY_TURNS: u64 = 10;
const HISTORY_TOOLS: u64 = 5;
const MEASURED_TOOLS: u64 = 20;
const COMMAND_RESULT_BYTES: usize = 4096;
const HISTORY_MCP_RESULT_BYTES: usize = 900_000;
const MEASURED_MCP_RESULT_BYTES: usize = 1_100_000;
const ANSWER_SENTENCES: usize = 60;
const ANSWER_SENTENCE: &str = "The session view keeps the reply text and bounds a tool's output. ";

fn mixed(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn output(turn: u64, tool: u64, bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 128);
    let mut line = 0u64;
    while text.len() < bytes {
        let seed = turn * 100_003 + tool * 10_007 + line * 101;
        text.push_str(&format!(
            "{:06} src/module-{:02}.ts operation=transfer-{}-{} cursor={} digest={:016x}{:016x} status=completed\n",
            line + 1,
            (tool + line) % 17,
            turn + 1,
            tool + 1,
            mixed(seed) as u32,
            mixed(seed + 1),
            mixed(seed + 2)
        ));
        line += 1;
    }
    text.truncate(bytes);
    text
}

fn answer(turn: u64) -> String {
    format!("Completed claudeAgent inspection {turn}. {}", ANSWER_SENTENCE.repeat(ANSWER_SENTENCES))
}

fn turn(index: u64, measured: bool) -> Vec<Value> {
    let (tools, mcp_bytes) = if measured { (MEASURED_TOOLS, MEASURED_MCP_RESULT_BYTES) } else { (HISTORY_TOOLS, HISTORY_MCP_RESULT_BYTES) };
    let frame = |kind: &str, content: Value| json!({"type": kind, "session_id": SESSION, "message": {"role": kind, "content": content}});
    let mut frames = vec![frame("user", json!(format!("Inspect transfer {index}")))];
    for tool in 0..=tools {
        let id = format!("tool-{index}-{tool}");
        let (name, input, bytes) = if tool == tools { ("mcp__test__search", json!({"query": "transfer"}), mcp_bytes) } else { ("Bash", json!({"command": format!("cat src/module-{tool}.ts")}), COMMAND_RESULT_BYTES) };
        frames.push(frame("assistant", json!([{"type": "tool_use", "id": id, "name": name, "input": input}])));
        frames.push(frame("user", json!([{"type": "tool_result", "tool_use_id": id, "content": output(index, tool, bytes)}])));
    }
    frames.push(frame("assistant", json!([{"type": "text", "text": answer(index)}])));
    frames.push(json!({"type": "result", "subtype": "success", "session_id": SESSION}));
    frames
}

fn write(path: &Path, frames: &[Value]) -> Result<u64, String> {
    let mut file = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    frames.iter().try_for_each(|frame| writeln!(file, "{frame}")).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::metadata(path).map(|meta| meta.len()).map_err(|e| e.to_string())
}

pub fn fixture(state: &Path) -> Result<Value, String> {
    let session = state.join("north/fleet").join(SESSION);
    if session.exists() {
        return Err(format!("{} already exists; pick a new --state", session.display()));
    }
    fs::create_dir_all(session.join("g1")).map_err(|e| format!("{}: {e}", session.display()))?;
    let record = json!({"name": SESSION, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": SESSION, "host_dir": session, "stream_offset": 0});
    fs::write(session.join("session.json"), record.to_string()).map_err(|e| e.to_string())?;
    let history: Vec<Value> = (0..HISTORY_TURNS).flat_map(|index| turn(index, false)).collect();
    let stream = session.join("g1/stream.jsonl");
    let measured = state.join("measured-turn.jsonl");
    let manifest = json!({
        "session": SESSION, "stream": stream, "measured_turn": measured,
        "history": {"turns": HISTORY_TURNS, "tools_per_turn": HISTORY_TOOLS, "command_result_bytes": COMMAND_RESULT_BYTES, "mcp_result_bytes": HISTORY_MCP_RESULT_BYTES, "stream_bytes": write(&stream, &history)?},
        "measured": {"tools": MEASURED_TOOLS, "command_result_bytes": COMMAND_RESULT_BYTES, "mcp_result_bytes": MEASURED_MCP_RESULT_BYTES, "stream_bytes": write(&measured, &turn(HISTORY_TURNS, true))?},
        "answer_bytes": answer(0).len(),
        "north_commands": {
            "load_the_thread": format!("XDG_STATE_HOME={} north fleet show {SESSION}", state.display()),
            "follow_the_measured_turn": format!("XDG_STATE_HOME={} north fleet show {SESSION} --follow, while {} is appended to {}", state.display(), measured.display(), stream.display()),
        },
    });
    fs::write(state.join("fixture.json"), format!("{manifest:#}\n")).map_err(|e| e.to_string())?;
    Ok(manifest)
}
