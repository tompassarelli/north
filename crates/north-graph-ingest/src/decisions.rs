use std::path::{Path, PathBuf};
use std::process::Command;

use north_graph::Telling;
use serde_json::Value;

use crate::facts::{Facts, instant, text};
use crate::places::Places;

const WRITER: &str = "decision-log";

pub fn read(places: &Places) -> Result<Telling, String> {
    let root = places.decisions.as_deref().ok_or("the decision log is off (NORTH_DECISION_LOG is empty)")?;
    let mut facts = Facts::new(&[]);
    for file in hour_files(root) {
        for line in lines(&file)? {
            if let Ok(record) = serde_json::from_str::<Value>(&line) {
                tell_decision(&mut facts, &record);
            }
        }
    }
    Ok(facts.finish())
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    found.sort();
    found
}

fn hour_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for kind in entries(root).into_iter().filter(|path| path.is_dir()) {
        for day in entries(&kind).into_iter().filter(|path| path.is_dir()) {
            files.extend(entries(&day).into_iter().filter(|file| {
                let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                !name.contains("snapshots") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
            }));
        }
    }
    files
}

fn lines(file: &Path) -> Result<Vec<String>, String> {
    let body = if file.extension().is_some_and(|extension| extension == "zst") {
        let output = Command::new("zstd").args(["-q", "-d", "-c"]).arg(file).output().map_err(|why| format!("{}: zstd: {why}", file.display()))?;
        if !output.status.success() {
            return Err(format!("{}: zstd failed", file.display()));
        }
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        std::fs::read_to_string(file).map_err(|why| format!("{}: {why}", file.display()))?
    };
    Ok(body.lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect())
}

fn tell_decision(facts: &mut Facts, record: &Value) {
    let (Some(id), Some(kind)) = (record["id"].as_str(), record["kind"].as_str()) else { return };
    let node = format!("decision:{id}");
    facts.tell(WRITER, text(&node), "decision-kind", text(kind));
    if let Some(at) = record["at"].as_f64().and_then(instant) {
        facts.tell(WRITER, text(&node), "at", at);
    }
    for (key, predicate) in [("host", "host"), ("follows", "follows")] {
        if let Some(value) = record[key].as_str() {
            let object = if key == "follows" { format!("decision:{value}") } else { value.to_owned() };
            facts.tell(WRITER, text(&node), predicate, text(object));
        }
    }
    let case = &record["case"];
    let decision = &record["decision"];
    match kind {
        "land.pick" => {
            tell_text(facts, &node, "about-branch", &case["branch"]);
            tell_text(facts, &node, "about-commit", &case["head"]);
            tell_text(facts, &node, "pick", &decision["pick"]);
        }
        "land.batch" => {
            for ticket in case["tickets"].as_array().into_iter().flatten() {
                tell_text(facts, &node, "about-branch", &ticket["branch"]);
                tell_text(facts, &node, "about-commit", &ticket["head"]);
            }
        }
        "land.judge" => {
            for lane in case["included"].as_array().into_iter().flatten() {
                tell_text(facts, &node, "included", lane);
            }
            tell_text(facts, &node, "verdict", &Value::from(first_word(&case["verdict"])));
            tell_text(facts, &node, "outcome", &Value::from(first_word(decision)));
        }
        "placement" => {
            tell_text(facts, &node, "launched", &case["launch"]["name"]);
            tell_text(facts, &node, "account", &decision["target"]["account"]);
            tell_text(facts, &node, "outcome", &Value::from(first_word(&decision["outcome"])));
        }
        "admission" => tell_text(facts, &node, "outcome", &decision["reason"]),
        _ => {}
    }
}

fn tell_text(facts: &mut Facts, node: &str, predicate: &str, value: &Value) {
    if let Some(value) = value.as_str().filter(|value| !value.is_empty()) {
        facts.tell(WRITER, text(node), predicate, text(value));
    }
}

fn first_word(value: &Value) -> String {
    match value {
        Value::String(word) => word.clone(),
        Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
        _ => String::new(),
    }
}
