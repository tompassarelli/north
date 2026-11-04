//! Gemini usage rows from `agy -p /usage`, plus the Pro quota stop recorded in Antigravity's conversation stores.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::record::Reading;
use crate::foundation::clock::Timestamp;
use crate::usage::py;

const COMMAND: &str = "agy -p /usage --output-format json";
const LIMIT: Duration = Duration::from_secs(30);
const AUTH: &[u8] = b"Authentication required.";

pub(super) fn read() -> Vec<Reading> {
    let conversations = crate::usage::profiles::home().join(".gemini/antigravity-cli/conversations");
    read_with(&mut agy_output, &conversations, Timestamp::now()).unwrap_or_default()
}

fn read_with(output: &mut dyn FnMut() -> Option<Vec<u8>>, conversations: &Path, now: Timestamp) -> Option<Vec<Reading>> {
    let response: Value = serde_json::from_slice(&output()?).ok()?;
    let data = &response["command"]["data"];
    if data["groups"].as_array().is_none_or(|groups| groups.is_empty()) {
        return None;
    }
    let account = data["email"].as_str().filter(|email| !email.is_empty()).map_or_else(
        || "default".to_owned(),
        |email| Sha256::digest(email.as_bytes()).iter().take(4).map(|byte| format!("{byte:02x}")).collect(),
    );
    let stamp = py::isoformat(now);
    let mut rows = usage_rows(data, &account, &stamp);
    rows.extend(recorded_pro(conversations, &account, now, &stamp));
    Some(rows)
}

fn row(account: &str, name: &str, used: Value, window: Value, reset: Value, source: &str, now: &str) -> Reading {
    json!({"provider": "gemini", "account": account, "kind": "window", "name": name, "used_pct": used, "window_min": window,
        "resets_at": reset, "amount": null, "source": source, "ts": now})
    .as_object()
    .cloned()
    .unwrap_or_default()
}

fn utc(text: &str) -> Option<String> {
    Timestamp::parse_rfc3339(text).map(py::isoformat)
}

fn usage_rows(data: &Value, account: &str, now: &str) -> Vec<Reading> {
    let mut rows = Vec::new();
    for group in data["groups"].as_array().into_iter().flatten() {
        for bucket in group["buckets"].as_array().into_iter().flatten() {
            let used = bucket["remaining_fraction"].as_f64().filter(|remaining| (0.0..=1.0).contains(remaining)).map(|remaining| 100.0 * (1.0 - remaining));
            let window = bucket["window"].as_str().unwrap_or_default();
            let minutes = match window {
                "5h" => json!(300),
                "weekly" => json!(10080),
                _ => Value::Null,
            };
            let reset = bucket["reset_time"].as_str().and_then(utc);
            rows.push(row(account, &format!("{}/{window}", group["name"].as_str().unwrap_or_default()), json!(used), minutes, json!(reset), COMMAND, now));
        }
    }
    rows
}

fn agy_output() -> Option<Vec<u8>> {
    let mut child = Command::new("agy")
        .args(["-p", "/usage", "--output-format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .ok()?;
    let denied = Arc::new(AtomicBool::new(false));
    let streams: [(Box<dyn Read + Send>, bool); 2] = [(Box::new(child.stdout.take()?), true), (Box::new(child.stderr.take()?), false)];
    let readers: Vec<_> = streams
        .into_iter()
        .map(|(stream, keep)| {
            let denied = denied.clone();
            std::thread::spawn(move || drain(stream, keep, &denied))
        })
        .collect();
    let started = Instant::now();
    let status = loop {
        if denied.load(Ordering::Relaxed) || started.elapsed() >= LIMIT {
            let _ = rustix::process::kill_process_group(rustix::process::Pid::from_raw(i32::try_from(child.id()).ok()?)?, rustix::process::Signal::Kill);
            let _ = child.wait();
            return None;
        }
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut collected = Vec::new();
    for reader in readers {
        collected.extend(reader.join().ok()?);
    }
    if denied.load(Ordering::Relaxed) || !status.success() {
        return None;
    }
    Some(collected)
}

fn drain(mut stream: Box<dyn Read + Send>, keep: bool, denied: &AtomicBool) -> Vec<u8> {
    let (mut buffer, mut tail, mut kept) = ([0u8; 65536], Vec::new(), Vec::new());
    while let Ok(count) = stream.read(&mut buffer) {
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.windows(AUTH.len()).any(|window| window == AUTH) {
            denied.store(true, Ordering::Relaxed);
        }
        let keep_from = tail.len().saturating_sub(64);
        tail.drain(..keep_from);
        if keep {
            kept.extend_from_slice(&buffer[..count]);
        }
    }
    kept
}

enum Field {
    Varint(u64),
    Bytes(Vec<u8>),
}

fn varint(blob: &[u8], mut offset: usize) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *blob.get(offset).ok_or("truncated varint")?;
        offset += 1;
        value |= u64::from(byte & 127).checked_shl(shift).unwrap_or(0);
        if byte < 128 {
            return Ok((value, offset));
        }
    }
    Err("invalid varint".into())
}

fn protobuf_fields(blob: &[u8]) -> Result<Vec<(u64, Field)>, String> {
    let (mut offset, mut fields) = (0, Vec::new());
    while offset < blob.len() {
        let (tag, next) = varint(blob, offset)?;
        offset = next;
        match tag & 7 {
            0 => {
                let (value, next) = varint(blob, offset)?;
                offset = next;
                fields.push((tag >> 3, Field::Varint(value)));
            }
            2 => {
                let (size, next) = varint(blob, offset)?;
                let end = next.checked_add(usize::try_from(size).map_err(|_| "oversized field")?).filter(|end| *end <= blob.len()).ok_or("truncated field")?;
                fields.push((tag >> 3, Field::Bytes(blob[next..end].to_vec())));
                offset = end;
            }
            1 => offset += 8,
            5 => offset += 4,
            _ => return Err("invalid wire type".into()),
        }
    }
    Ok(fields)
}

fn last(fields: Vec<(u64, Field)>, number: u64) -> Option<Field> {
    fields.into_iter().rev().find(|(found, _)| *found == number).map(|(_, field)| field)
}

fn event_time(metadata: &[u8]) -> Result<Timestamp, String> {
    let Some(Field::Bytes(stamp)) = last(protobuf_fields(metadata)?, 1) else { return Err("no event stamp".into()) };
    let fields = protobuf_fields(&stamp)?;
    let mut seconds = None;
    let mut nanos = 0u64;
    for (number, field) in fields {
        match (number, field) {
            (1, Field::Varint(value)) => seconds = Some(value),
            (2, Field::Varint(value)) => nanos = value,
            _ => {}
        }
    }
    let seconds = seconds.ok_or("no event seconds")?;
    Ok(Timestamp::from_unix_nanos(i128::from(seconds) * 1_000_000_000 + i128::from(nanos)))
}

fn recorded_pro(conversations: &Path, account: &str, now: Timestamp, stamp: &str) -> Option<Reading> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(conversations).ok()?.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "db")).collect();
    paths.sort();
    let mut latest: Option<(Timestamp, String, String)> = None;
    for path in paths {
        let _ = scan(&path, now, &mut latest);
    }
    latest.map(|(_, source, reset)| row(account, "gemini-pro-agent/recorded-quota", json!(100), Value::Null, json!(reset), &source, stamp))
}

fn scan(path: &Path, now: Timestamp, latest: &mut Option<(Timestamp, String, String)>) -> Result<(), String> {
    let fail = |error: rusqlite::Error| error.to_string();
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail)?;
    db.busy_timeout(Duration::from_millis(100)).map_err(fail)?;
    let pattern = Regex::new(r#"quotaResetTimeStamp["\\:\s]*([0-9T:Z.+-]+)"#).map_err(|error| error.to_string())?;
    let mut statement = db
        .prepare(
            "SELECT idx, metadata, step_payload FROM steps WHERE step_type = 17 AND status = 3 AND instr(step_payload, ?1) > 0 \
             AND instr(step_payload, ?2) > 0 AND instr(step_payload, ?3) > 0",
        )
        .map_err(fail)?;
    let records = statement
        .query_map([b"quotaResetTimeStamp".as_slice(), b"gemini-pro-agent", b"429"], |record| Ok((record.get::<_, i64>(0)?, record.get::<_, Vec<u8>>(1)?, record.get::<_, Vec<u8>>(2)?)))
        .map_err(fail)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    for record in records {
        let (idx, metadata, payload) = record.map_err(fail)?;
        let text = String::from_utf8_lossy(&payload);
        let Some(reset) = pattern.captures(&text).and_then(|found| utc(&found[1])) else { continue };
        if Timestamp::parse_rfc3339(&reset).is_none_or(|reset| reset <= now) {
            continue;
        }
        let event = event_time(&metadata)?;
        if event > now {
            continue;
        }
        let source = format!("~/.gemini/antigravity-cli/conversations/{name} steps[idx={idx}].step_payload+metadata");
        let candidate = (event, source, reset);
        if latest.as_ref().is_none_or(|best| candidate > *best) {
            *latest = Some(candidate);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frozen() -> Timestamp {
        Timestamp::parse_rfc3339("2026-11-04T12:00:00Z").unwrap()
    }

    fn varint_bytes(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 127) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 128);
        }
    }

    fn metadata(seconds: u64) -> Vec<u8> {
        let mut stamp = vec![0x08];
        stamp.extend(varint_bytes(seconds));
        let mut blob = vec![0x0a];
        blob.extend(varint_bytes(stamp.len() as u64));
        blob.extend(stamp);
        blob
    }

    fn conversation(directory: &Path, name: &str, event: u64, reset: &str) {
        let db = Connection::open(directory.join(name)).unwrap();
        db.execute("CREATE TABLE steps(idx INTEGER, metadata BLOB, step_payload BLOB, step_type INTEGER, status INTEGER)", []).unwrap();
        let payload = format!("{{\"quotaResetTimeStamp\":\"{reset}\",\"model\":\"gemini-pro-agent\",\"code\":429}}");
        db.execute("INSERT INTO steps VALUES (7, ?1, ?2, 17, 3)", rusqlite::params![metadata(event), payload.as_bytes()]).unwrap();
    }

    const RESPONSE: &str = r#"{"command":{"data":{"email":"tom@example.com","groups":[{"name":"Gemini Models","buckets":[
        {"window":"5h","remaining_fraction":0.75,"reset_time":"2026-11-04T16:30:00+00:00"},
        {"window":"weekly","remaining_fraction":1.5,"reset_time":"2026-11-10T08:00:00Z"},
        {"window":"odd","remaining_fraction":true,"reset_time":"2026-11-10T08:00:00"}]}]}}}"#;

    #[test]
    fn buckets_become_window_rows_and_out_of_range_fractions_have_no_reading() {
        let empty = tempfile::tempdir().unwrap();
        let rows = read_with(&mut || Some(RESPONSE.as_bytes().to_vec()), empty.path(), frozen()).unwrap();
        let shown: Vec<_> = rows.iter().map(|row| (row["name"].as_str().unwrap().to_owned(), row["used_pct"].clone(), row["window_min"].clone(), row["resets_at"].clone())).collect();
        assert_eq!(
            shown,
            [
                ("Gemini Models/5h".to_owned(), json!(25.0), json!(300), json!("2026-11-04T16:30:00Z")),
                ("Gemini Models/weekly".to_owned(), Value::Null, json!(10080), json!("2026-11-10T08:00:00Z")),
                ("Gemini Models/odd".to_owned(), Value::Null, Value::Null, Value::Null),
            ]
        );
        assert_eq!(rows[0]["account"], "72bb75a9");
        assert_eq!(rows[0]["source"], COMMAND);
    }

    #[test]
    fn a_response_without_groups_or_output_yields_nothing() {
        let empty = tempfile::tempdir().unwrap();
        assert!(read_with(&mut || Some(br#"{"command":{"data":{"groups":[]}}}"#.to_vec()), empty.path(), frozen()).is_none());
        assert!(read_with(&mut || None, empty.path(), frozen()).is_none());
    }

    #[test]
    fn the_latest_recorded_pro_quota_stop_still_in_force_becomes_a_full_window() {
        let directory = tempfile::tempdir().unwrap();
        let now = frozen().unix_seconds() as u64;
        conversation(directory.path(), "a.db", now - 3600, "2026-11-04T18:00:00Z");
        conversation(directory.path(), "b.db", now - 600, "2026-11-04T20:00:00Z");
        conversation(directory.path(), "c.db", now - 60, "2026-11-04T11:00:00Z");
        conversation(directory.path(), "d.db", now + 600, "2026-11-04T21:00:00Z");
        let rows = read_with(&mut || Some(RESPONSE.as_bytes().to_vec()), directory.path(), frozen()).unwrap();
        let pro = rows.last().unwrap();
        assert_eq!(
            (pro["name"].as_str(), pro["used_pct"].clone(), pro["resets_at"].as_str(), pro["source"].as_str()),
            (Some("gemini-pro-agent/recorded-quota"), json!(100), Some("2026-11-04T20:00:00Z"), Some("~/.gemini/antigravity-cli/conversations/b.db steps[idx=7].step_payload+metadata"))
        );
    }

    #[test]
    fn protobuf_fields_reject_truncated_and_unknown_encodings() {
        assert!(protobuf_fields(&[0x0a, 0x05, 0x01]).is_err());
        assert!(protobuf_fields(&[0x0b]).is_err());
        assert!(varint(&[0x80; 11], 0).is_err());
    }
}
