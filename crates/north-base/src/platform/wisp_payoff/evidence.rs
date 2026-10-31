use super::Run;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn discover(root: &Path, files: &mut BTreeSet<PathBuf>, warnings: &mut Vec<String>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            warnings.push(format!("{}: {error}", root.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            warnings.push(format!("cannot stat {}", path.display()));
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            discover(&path, files, warnings);
        } else if matches!(
            entry.file_name().to_str(),
            Some("batch.json" | "report.json")
        ) {
            files.insert(path);
        }
    }
}
fn number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(0.0)
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn family(script: &str) -> String {
    let path = Path::new(script);
    let parent = path.parent().unwrap_or(Path::new(""));
    let name = parent
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    if name == "pads" {
        format!(
            "pad/{}",
            path.file_stem().unwrap_or_default().to_string_lossy()
        )
    } else {
        format!("pad/{name}")
    }
}
pub(super) fn read(path: &Path) -> Result<Vec<Run>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    let fallback = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    Ok(decode(path, &value, fallback))
}
pub(super) fn decode(path: &Path, value: &Value, fallback: i64) -> Vec<Run> {
    if value["headless_session"].as_bool() == Some(true) {
        return Vec::new();
    }
    if let Some(scripts) = value["scripts"].as_array() {
        if !value["fresh_each"].is_boolean() || !value["hot"].is_boolean() {
            return Vec::new();
        }
        let Some(pairs) = value["pairs"].as_array() else {
            return Vec::new();
        };
        return scripts
            .iter()
            .filter(|s| {
                !text(s, "script").is_empty()
                    && text(s, "pair") != "session"
                    && pairs
                        .iter()
                        .any(|pair| pair.as_str() == Some(text(s, "pair")))
            })
            .map(|s| pad_run(path, s, fallback))
            .collect();
    }
    let Some(results) = value["results"].as_array() else {
        return Vec::new();
    };
    let results: Vec<_> = results
        .iter()
        .filter(|s| !text(s, "id").is_empty() && text(s, "verdict") != "skipped")
        .collect();
    let started = value["started"].as_i64().unwrap_or(0);
    let finished = value["finished"].as_i64().unwrap_or(0);
    let elapsed = (finished - started).max(0) as f64 / 30_000.0;
    results
        .iter()
        .filter(|s| !text(s, "id").is_empty())
        .map(|s| Run {
            path: path.into(),
            kind: format!("accept/{}", text(s, "id")),
            script: None,
            at: if started > 0 {
                started / 1000
            } else {
                fallback
            },
            client_minutes: elapsed / results.len().max(1) as f64,
            estimated: true,
            agreement: None,
            invalid: text(s, "verdict") == "invalid",
        })
        .collect()
}

fn pad_run(path: &Path, s: &Value, fallback: i64) -> Run {
    let verdict = text(s, "verdict");
    let agreement = if number(s, "compare") > 0.0
        && number(s, "headless") > 0.0
        && !text(s, "summary").starts_with("compare failed:")
    {
        match verdict {
            "PASS" => Some(true),
            "FAIL" => Some(false),
            _ => None,
        }
    } else {
        None
    };
    Run {
        path: path.into(),
        kind: family(text(s, "script")),
        script: Some(text(s, "script").into()),
        at: fallback,
        client_minutes: ["game", "reset", "reload", "run"]
            .iter()
            .map(|key| number(s, key))
            .sum::<f64>()
            / 30.0,
        estimated: false,
        agreement,
        invalid: verdict == "INVALID",
    }
}
