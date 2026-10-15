pub mod common;
pub mod compose;
pub mod distill;
pub mod score;
pub mod store;
use compose as render;
mod live;
mod sources;
mod tierb;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
pub use common::{redact, state_dir};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn re(pattern: &str) -> Regex {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, Regex>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry(pattern.to_owned())
        .or_insert_with(|| Regex::new(pattern).unwrap())
        .clone()
}
fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}
fn env_path(name: &str, fallback: PathBuf) -> PathBuf {
    env::var_os(name).map(PathBuf::from).unwrap_or(fallback)
}
fn north_dir() -> PathBuf {
    env_path("NORTH_GROUNDING_NORTH", home().join("code/north/main"))
}
fn handoffs_dir() -> PathBuf {
    env_path(
        "NORTH_GROUNDING_HANDOFFS",
        home().join(".local/state/agents/handoffs"),
    )
}
fn convo_path() -> PathBuf {
    env_path(
        "NORTH_GROUNDING_CONVO_DB",
        home().join(".local/state/convo/index-v4.db"),
    )
}
fn s(v: &Value, key: &str) -> String {
    v[key].as_str().map(str::to_owned).unwrap_or_else(|| {
        if v[key].is_null() {
            String::new()
        } else {
            v[key].to_string()
        }
    })
}
fn num(v: &Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| v[key].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0))
}
fn mtime(path: &Path) -> f64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map_or(0.0, |t| Timestamp::from(t).unix_seconds_f64())
}
fn ulen(s: &str) -> usize {
    s.encode_utf16().count()
}
fn slice(s: &str, start: usize, len: usize) -> String {
    String::from_utf16_lossy(&s.encode_utf16().skip(start).take(len).collect::<Vec<_>>())
}
fn cut(s: &str, len: usize) -> String {
    slice(s, 0, len)
}
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
fn display(path: &Path) -> String {
    let p = path.to_string_lossy();
    let h = format!("{}/", home().display());
    if p.starts_with(&h) {
        format!("~/{}", &p[h.len()..])
    } else {
        p.into_owned()
    }
}
fn epoch(s: &str) -> f64 {
    Timestamp::parse_loose(&s.replace(' ', "T")).map_or(0.0, |t| t.unix_millis() as f64 / 1000.0)
}
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|s| s == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}
fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|s| s == name)
}
fn command(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output()?;
    if !out.status.success() {
        return Err(format!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
pub fn main(args: &[String]) -> NorthResult<i32> {
    if args.first().is_some_and(|a| a == "distill") {
        return distill::main(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "score") {
        return score::main(&args[1..]);
    }
    run(args).map_err(|e| NorthError::Configuration(e.to_string()))
}
fn run(args: &[String]) -> Result<i32> {
    if has(args, "--help") || has(args, "-h") {
        println!(
            "usage: north grounding query [--query TEXT] [--tier A|B] | refresh [--force] [--if-idle] [--source NAME]... | hook | distill | score"
        );
        return Ok(0);
    }
    if args.first().is_some_and(|a| a == "refresh") {
        let only = args
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if a == "--source" {
                    args.get(i + 1).cloned()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            sources::refresh(&only, has(args, "--force"), !has(args, "--if-idle"))?
        );
        return Ok(0);
    }
    if args.first().is_some_and(|a| a == "hook") {
        let mut raw = String::new();
        std::io::stdin().take(1048577).read_to_string(&mut raw)?;
        if ulen(&raw) > 1048576 {
            return Ok(0);
        }
        let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
            return Ok(65);
        };
        if !payload.is_object()
            || payload.get("agent_id").is_some()
            || payload.get("agent_type").is_some()
            || payload
                .get("hook_event_name")
                .is_some_and(|v| v != "UserPromptSubmit")
        {
            return Ok(0);
        }
        sources::spawn_refresh_if_stale();
        let text = render::block(
            &s(&payload, "prompt"),
            "A",
            Timestamp::now().unix_seconds_f64(),
            &prior_reply(&s(&payload, "transcript_path")),
        )?;
        if !text.is_empty() {
            println!(
                "{}",
                json!({"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":text}})
            );
        }
        return Ok(0);
    }
    let query = flag(args, "--query")
        .or_else(|| flag(args, "-q"))
        .unwrap_or("");
    let text = render::block(
        query,
        flag(args, "--tier").unwrap_or("A"),
        Timestamp::now().unix_seconds_f64(),
        &env::var("NORTH_GROUNDING_PRIOR").unwrap_or_default(),
    )?;
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(0)
}
fn tail(path: &Path, limit: usize) -> String {
    use std::io::{Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return String::new();
    };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size.saturating_sub(limit as u64)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}
fn prior_reply(path: &str) -> String {
    for line in tail(Path::new(path), 1048576).lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["type"] != "assistant" || v["message"]["role"] != "assistant" {
            continue;
        }
        let content = &v["message"]["content"];
        let text = if let Some(t) = content.as_str() {
            t.to_owned()
        } else if let Some(parts) = content.as_array() {
            parts
                .iter()
                .filter(|v| v["type"] == "text")
                .map(|v| s(v, "text"))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        if !text.is_empty() {
            return cut(&text, 4000);
        }
    }
    String::new()
}
