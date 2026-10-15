use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use toml_edit::DocumentMut;

const LOOKBACK: Duration = Duration::from_secs(15 * 60);
const TAIL_BYTES: u64 = 1024 * 1024;

pub(crate) fn rate_limit_stop(line: &str) -> Option<(String, String)> {
    if !line.contains("rate_limit") && !line.contains("429") {
        return None;
    }
    let entry: Value = serde_json::from_str(line).ok()?;
    if entry["isApiErrorMessage"] != Value::Bool(true)
        || (entry["error"] != "rate_limit" && entry["apiErrorStatus"] != 429)
    {
        return None;
    }
    let stamp = entry["timestamp"].as_str()?;
    let valid = stamp.len() >= 19
        && stamp.as_bytes().iter().take(19).enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        });
    if !valid {
        return None;
    }
    let text = entry["message"]["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find_map(|block| block["text"].as_str()))
        .unwrap_or("")
        .chars()
        .take(160)
        .collect::<String>();
    Some((format!("{}Z", &stamp[..19]), text))
}

fn transcripts(root: &Path, depth: usize, since: SystemTime, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() && depth > 0 {
            transcripts(&path, depth - 1, since, found);
        } else if kind.is_file()
            && path.extension().is_some_and(|extension| extension == "jsonl")
            && entry.metadata().and_then(|meta| meta.modified()).is_ok_and(|modified| modified >= since)
        {
            found.push(path);
        }
    }
}

fn tail(path: &Path) -> String {
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let length = file.metadata().map_or(0, |meta| meta.len());
    let start = length.saturating_sub(TAIL_BYTES);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(crate) fn newest_stop(config_dir: &Path, now: SystemTime) -> Option<(String, String)> {
    let mut found = Vec::new();
    transcripts(&config_dir.join("projects"), 3, now.checked_sub(LOOKBACK)?, &mut found);
    found.iter().flat_map(|path| tail(path).lines().filter_map(rate_limit_stop).collect::<Vec<_>>()).max()
}

pub(crate) fn usage_row(account: &str, stamp: &str, text: &str) -> Value {
    let name = if text.contains("weekly limit") { "weekly_all" } else { "session" };
    json!({"type": "usage", "ts": stamp, "provider": "claude", "account": account, "kind": "throttle",
           "name": name, "used_pct": null, "window_min": if name == "session" { json!(300) } else { json!(10080) },
           "resets_at": null, "amount": null,
           "source": format!("429 rate_limit transcript stop: {text}")})
}

pub(crate) fn host_profiles(orchestration: &str, host: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let Ok(document) = orchestration.parse::<DocumentMut>() else { return Vec::new() };
    let Some(profiles) = document
        .get("accounts")
        .and_then(|item| item.get("claude"))
        .and_then(|item| item.get("profiles"))
        .and_then(|item| item.as_table_like())
    else {
        return Vec::new();
    };
    profiles
        .iter()
        .filter_map(|(label, hosts)| {
            let entry = hosts.as_table_like()?.get(host)?;
            let path = entry.as_table_like().and_then(|table| table.get("path")).and_then(|path| path.as_str());
            let directory = match path {
                Some(path) => path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest)),
                None => home.join(".claude"),
            };
            Some((label.to_owned(), directory))
        })
        .collect()
}

fn configured_profiles(home: &Path) -> Vec<(String, PathBuf)> {
    let path = std::env::var_os("AGENTS_ORCHESTRATION").map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("AGENTS_OPERATOR_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("code/south/main/agents"))
            .join("orchestration.toml")
    });
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    host_profiles(&fs::read_to_string(path).unwrap_or_default(), host.trim(), home)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let usage = "Usage: north usage-throttle [--profile LABEL=DIR]... [--state FILE] [--threads PATH]";
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unset")?);
    let mut profiles = Vec::new();
    let mut state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("agents/usage/claude-throttle.json");
    let mut threads = PathBuf::from("threads");
    let mut arguments = arguments.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or(usage)?;
        match flag.as_str() {
            "--profile" => {
                let (label, directory) = value.split_once('=').ok_or(usage)?;
                profiles.push((label.to_owned(), PathBuf::from(directory)));
            }
            "--state" => state = PathBuf::from(value),
            "--threads" => threads = PathBuf::from(value),
            _ => return Err(usage.into()),
        }
    }
    if profiles.is_empty() {
        profiles = configured_profiles(&home);
    }
    let mut seen: BTreeMap<String, String> =
        fs::read_to_string(&state).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let now = SystemTime::now();
    let mut rows = Vec::new();
    for (label, directory) in profiles {
        if let Some((stamp, text)) = newest_stop(&directory, now)
            && seen.get(&label).is_none_or(|previous| *previous < stamp)
        {
            rows.push(usage_row(&label, &stamp, &text));
            seen.insert(label, stamp);
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    let payload: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let mut child = Command::new(&threads)
        .arg("ingest")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("usage-throttle: {}: {error}", threads.display()))?;
    child.stdin.take().ok_or("usage-throttle: no ingest stdin")?.write_all(payload.as_bytes()).map_err(|error| error.to_string())?;
    let status = child.wait().map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("usage-throttle: threads ingest exited {status}"));
    }
    if let Some(parent) = state.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&state, serde_json::to_string(&seen).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    print!("{payload}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOP: &str = r#"{"type":"assistant","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"timestamp":"2026-10-15T08:08:19.088Z","message":{"model":"<synthetic>","content":[{"type":"text","text":"You've hit your session limit · resets 7:20pm (Asia/Taipei)"}]}}"#;

    #[test]
    fn rate_limit_stop_reads_the_synthetic_429_entry_only() {
        assert_eq!(
            rate_limit_stop(STOP),
            Some(("2026-10-15T08:08:19Z".into(), "You've hit your session limit · resets 7:20pm (Asia/Taipei)".into()))
        );
        assert_eq!(rate_limit_stop(&STOP.replace(r#""isApiErrorMessage":true"#, r#""isApiErrorMessage":false"#)), None);
        assert_eq!(rate_limit_stop(&STOP.replace(r#""error":"rate_limit","apiErrorStatus":429"#, r#""error":"overloaded","apiErrorStatus":529"#)), None);
        assert_eq!(rate_limit_stop(r#"{"type":"user","message":{"content":"grep rate_limit 429"}}"#), None);
        assert_eq!(rate_limit_stop(&STOP.replace("2026-10-15T08:08:19.088Z", "soon")), None);
    }

    #[test]
    fn usage_row_names_the_window_and_marks_429() {
        let row = usage_row("a", "2026-10-15T08:08:19Z", "You've hit your session limit · resets 7:20pm");
        assert_eq!((row["name"].as_str(), row["kind"].as_str(), row["account"].as_str()), (Some("session"), Some("throttle"), Some("a")));
        assert!(row["source"].as_str().unwrap().starts_with("429 rate_limit"));
        assert_eq!(usage_row("a", "t", "You've hit your weekly limit · resets Jul 25")["name"], "weekly_all");
    }

    #[test]
    fn host_profiles_resolve_this_hosts_directories() {
        let toml = "[accounts.claude]\nprofiles = { a = { nexus = {}, whiterabbit = { path = \"~/.local/state/north/profiles/claude-a\" } }, b = { whiterabbit = {} } }\n";
        let home = Path::new("/home/t");
        assert_eq!(
            host_profiles(toml, "whiterabbit", home),
            vec![("a".into(), PathBuf::from("/home/t/.local/state/north/profiles/claude-a")), ("b".into(), PathBuf::from("/home/t/.claude"))]
        );
        assert_eq!(host_profiles(toml, "nexus", home), vec![("a".into(), PathBuf::from("/home/t/.claude"))]);
        assert_eq!(host_profiles(toml, "elsewhere", home), vec![]);
    }

    #[test]
    fn newest_stop_takes_the_latest_recent_transcript_stop() {
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join("projects/-home-t/session/subagents");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("agent.jsonl"), format!("{STOP}\n{}\n", STOP.replace("08:08:19", "08:09:02"))).unwrap();
        fs::write(directory.path().join("projects/-home-t/main.jsonl"), format!("{}\n", STOP.replace("08:08:19", "07:00:00"))).unwrap();
        assert_eq!(newest_stop(directory.path(), SystemTime::now()).map(|stop| stop.0), Some("2026-10-15T08:09:02Z".into()));
        assert_eq!(newest_stop(directory.path(), SystemTime::now() + Duration::from_secs(16 * 60)), None);
    }
}
