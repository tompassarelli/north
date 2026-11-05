use super::briefs::Event;
use super::{World, capacity_home, home};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const ACTIVE_SECONDS: f64 = 300.0;

fn py(value: &Value) -> pyjson::Value {
    pyjson::loads(value.to_string().as_bytes()).unwrap_or(pyjson::Value::Null)
}

fn head(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn append(path: &Path, line: &pyjson::Value) {
    let _ = path.parent().map(std::fs::create_dir_all);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", pyjson::dumps(line));
    }
}

fn input_type(value: Option<&Value>) -> &'static str {
    match value {
        None | Some(Value::Null) => "NoneType",
        Some(Value::Bool(_)) => "bool",
        Some(Value::Number(number)) if number.is_f64() => "float",
        Some(Value::Number(_)) => "int",
        Some(Value::String(_)) => "str",
        Some(Value::Array(_)) => "list",
        Some(Value::Object(_)) => "dict",
    }
}

pub(super) fn denial(world: &impl World, event: &Event, gate: &str, reason: Option<&str>) {
    let state = world.var("XDG_STATE_HOME").map_or_else(|| home(world).join(".local/state"), PathBuf::from);
    let heads = event.texts.iter().take(4).map(|text| pyjson::Value::from(head(text, 160))).collect();
    append(&state.join("agents/spawn-gate-denials.jsonl"), &pyjson::Value::object([
        ("at", Timestamp::from_unix_seconds_f64(world.now()).iso().into()),
        ("gate", gate.into()),
        ("tool", py(&event.raw["tool_name"])),
        ("input_type", input_type(event.raw.get("tool_input")).into()),
        ("heads", pyjson::Value::Array(heads)),
        ("reason", reason.map_or(pyjson::Value::Null, Into::into)),
    ]));
}

pub(super) fn urgent(world: &impl World, event: &Event, fact: &str) {
    let root = world.var("CODEX_BEHAVIOR_STATE").map_or_else(
        || PathBuf::from(world.var("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into())).join("codex-behavior"),
        PathBuf::from,
    );
    append(&root.join("verify-overrides.jsonl"), &pyjson::Value::object([
        ("at", pyjson::Value::float(world.now())),
        ("session", py(&event.raw["session_id"])),
        ("agent", py(&event.raw["agent_id"])),
        ("gate", "spawn-capacity".into()),
        ("case", "URGENT".into()),
        ("cwd", py(&event.raw["cwd"])),
        ("fact", head(fact, 300).into()),
    ]));
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect()
}

fn recent(directory: &Path, cutoff: f64) -> usize {
    entries(directory)
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".jsonl")))
        .filter_map(|path| std::fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok())
        .filter(|modified| modified.as_secs_f64() >= cutoff)
        .count()
}

pub(super) fn local_workers(world: &impl World) -> usize {
    let home = capacity_home(world);
    let cutoff = world.now() - ACTIVE_SECONDS;
    let claude: usize = entries(&home.join(".claude/projects")).iter().flat_map(|project| entries(project)).map(|session| recent(&session.join("subagents"), cutoff)).sum();
    claude + recent(&home.join(".codex/sessions").join(world.today()), cutoff)
}
