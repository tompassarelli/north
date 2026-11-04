use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use toml_edit::{DocumentMut, Item};

use super::{Away, IdleRow, Label, Machine, Moment, Signals, Turn, WINDOW_DAYS, Zone, iso, parse};

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn env_path(env: Env, name: &str) -> Option<PathBuf> {
    env(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).filter(Value::is_object).unwrap_or(Value::Null)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn human(row: &Value) -> bool {
    row.get("origin").and_then(|origin| origin.get("kind")).and_then(Value::as_str) == Some("human")
}

type TurnsByFile = BTreeMap<String, (BTreeMap<i64, String>, u64)>;

fn indexed(index: &Path, home: &Path, start: &str, end: &str) -> rusqlite::Result<TurnsByFile> {
    let db = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut query = db.prepare(
        "SELECT f.path,m.line,m.ts,f.off FROM files f JOIN msg m INDEXED BY msg_file ON m.file_id=f.id \
         WHERE f.provider='anthropic' AND f.agent IS NULL AND f.cwd=? AND m.role='user' AND m.ts>=? AND m.ts<=?",
    )?;
    let mut grouped: TurnsByFile = BTreeMap::new();
    let rows = query.query_map((home.to_string_lossy(), start, end), |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)?)))?;
    for row in rows {
        let (path, line, at, offset): (String, i64, String, i64) = row?;
        let entry = grouped.entry(path).or_default();
        entry.0.insert(line, at);
        entry.1 = u64::try_from(offset).unwrap_or(0);
    }
    Ok(grouped)
}

fn transcript_turns(name: &str, wanted: &BTreeMap<i64, String>, offset: u64, now: Moment, turns: &mut Vec<Turn>) -> Option<()> {
    let mut file = fs::File::open(name).ok()?;
    let last = *wanted.keys().next_back()?;
    for (number, line) in BufReader::new(&mut file).split(b'\n').enumerate() {
        let number = number as i64 + 1;
        if let Some(at) = wanted.get(&number) {
            let row: Value = serde_json::from_slice(&line.ok()?).ok()?;
            let exact = human(&row) && row["origin"].is_object();
            let flagged = |key: &str| row.get(key).is_some_and(truthy);
            let plain = row.get("origin").is_none_or(Value::is_null) && !flagged("isMeta") && !flagged("isCompactSummary");
            if (exact || plain)
                && let Some(at) = parse(at)
            {
                turns.push(Turn { at, label: if exact { Label::Exact } else { Label::Heuristic } });
            }
        }
        if number >= last {
            break;
        }
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    for line in tail.split(|byte| *byte == b'\n') {
        let Ok(row) = serde_json::from_slice::<Value>(line) else { continue };
        if human(&row)
            && let Some(at) = row.get("timestamp").filter(|stamp| truthy(stamp)).and_then(Value::as_str).and_then(parse)
            && now - Duration::days(*WINDOW_DAYS) <= at
            && at <= now
        {
            turns.push(Turn { at, label: Label::Exact });
        }
    }
    Some(())
}

pub fn human_turns(now: Moment, index: &Path, home: &Path) -> Vec<Turn> {
    if !index.is_file() {
        return Vec::new();
    }
    let Ok(grouped) = indexed(index, home, &iso(now - Duration::days(*WINDOW_DAYS)), &iso(now)) else { return Vec::new() };
    let mut turns = Vec::new();
    for (name, (wanted, offset)) in &grouped {
        transcript_turns(name, wanted, *offset, now, &mut turns);
    }
    let cutoff = turns.iter().filter(|turn| turn.label == Label::Exact).map(|turn| turn.at).min().unwrap_or(now);
    turns.retain(|turn| turn.label == Label::Exact || turn.at < cutoff);
    turns.sort_by_key(|turn| turn.at);
    turns
}

fn away(operator: &Path) -> Vec<Away> {
    let Some(document) = fs::read_to_string(operator.join("presence.toml")).ok().and_then(|text| text.parse::<DocumentMut>().ok()) else { return Vec::new() };
    let text = |item: &Item| item.as_str().map(str::to_owned).or_else(|| item.as_datetime().map(ToString::to_string));
    let Some(rows) = document.get("away").and_then(Item::as_array_of_tables) else { return Vec::new() };
    rows.iter()
        .filter_map(|row| {
            let moment = |key: &str| row.get(key).and_then(text).and_then(|value| parse(&value));
            let field = |key: &str| row.get(key).and_then(Item::as_str).unwrap_or_default().to_owned();
            Some(Away { from: moment("from")?, to: moment("to")?, kind: field("kind"), said: field("said") })
        })
        .collect()
}

fn idle_history(state: &Path, now: Moment) -> Vec<IdleRow> {
    let Ok(text) = fs::read_to_string(state.join("agents/presence/idle.jsonl")) else { return Vec::new() };
    text.lines()
        .filter_map(|line| {
            let row: Value = serde_json::from_str(line).ok()?;
            let at = parse(row.get("at")?.as_str()?)?;
            (now - Duration::days(*WINDOW_DAYS) <= at && at <= now).then(|| IdleRow { at, idle: row.get("idle").and_then(Value::as_bool) })
        })
        .collect()
}

fn desktop(env: Env, now: Moment, proc: &Path) -> Option<Machine> {
    let runtime = env_path(env, "XDG_RUNTIME_DIR").unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw())));
    let seat = read_json(&runtime.join("desktop-idle.json"));
    let pid = match &seat["pid"] {
        Value::Number(number) => number.as_f64()? as i64,
        Value::String(text) => text.trim().parse().ok()?,
        _ => return None,
    };
    let stat = fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    let (name, fields) = stat.split_once('(')?.1.rsplit_once(')')?;
    let fields: Vec<&str> = fields.split_whitespace().collect();
    let started = *fields.get(19)?;
    let alive = !matches!(*fields.first()?, "Z" | "X" | "T" | "t");
    if name != "swayidle" || !alive || seat["started"].as_str() != Some(started) {
        return None;
    }
    let mut machine = Machine { active: Some(seat["idle"] == Value::Bool(false)), at: Some(now), ..Machine::default() };
    let boot = fs::read_to_string(proc.join("stat")).ok().and_then(|text| {
        text.lines().find_map(|line| line.strip_prefix("btime ")).and_then(|value| value.split_whitespace().next()?.parse::<i64>().ok())
    });
    if let (Some(boot), Ok(ticks)) = (boot, started.parse::<i64>()) {
        // Linux /proc reports process start times in USER_HZ, fixed at 100 ticks per second.
        let nanos = i128::from(boot) * 1_000_000_000 + i128::from(ticks) * 10_000_000;
        machine.session_start = OffsetDateTime::from_unix_timestamp_nanos(nanos).ok();
    }
    Some(machine)
}

fn laptop(env: Env, home: &Path) -> Option<Machine> {
    let path = env_path(env, "LAPTOP_STATE").unwrap_or_else(|| home.join(".local/state/placement/laptop.json"));
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let seen = read_json(&path.with_file_name(format!("{stem}-observations.json")));
    let (active, idle) = (truthy(&seen["active"]), truthy(&seen["idle"]));
    if !truthy(&seen["at"]) || active == idle {
        return None;
    }
    let at = parse(seen["at"].as_str()?)?;
    let idle_since = if idle { seen["idle"].as_str().and_then(parse) } else { None };
    Some(Machine { at: Some(at), active: Some(active), session_start: None, idle_since })
}

pub fn live_signals(env: Env, now: Moment, proc: &Path) -> Signals {
    let home = env_path(env, "HOME").unwrap_or_default();
    let state = env_path(env, "XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
    let operator = env_path(env, "AGENTS_OPERATOR_DIR").unwrap_or_else(|| home.join("code/south/main/agents"));
    let index = env_path(env, "CONVO_STATE").unwrap_or_else(|| state.join("convo")).join("index-v4.db");
    let timezone = env("TZ").filter(|name| !name.is_empty()).and_then(|name| Zone::named(&name)).unwrap_or_else(|| Zone::local(now));
    Signals {
        turns: human_turns(now, &index, &home),
        idle: idle_history(&state, now),
        away: away(&operator),
        timezone: Some(timezone),
        desktop: desktop(env, now, proc),
        laptop: laptop(env, &home),
    }
}
