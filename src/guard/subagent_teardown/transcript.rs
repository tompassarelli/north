use crate::pyjson::{self, Value};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::sync::LazyLock;

const LINE_LIMIT: usize = 16 << 20;
const DEFAULT_MONITOR_MS: f64 = 300_000.0;

static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:with ID: |\(task )([A-Za-z0-9_-]+)").unwrap());
static OUTPUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Output is being written to: (\S+?\.output)").unwrap());
static NOTICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<task-id>([A-Za-z0-9_-]+)</task-id>").unwrap());
static STAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^([0-9]{4})-(1[0-2]|0[1-9]|[1-9])-(3[0-1]|[1-2][0-9]|0[1-9]|[1-9]| [1-9])",
        r"T(2[0-3]|[0-1][0-9]|[0-9]):([0-5][0-9]|[0-9]):(6[0-1]|[0-5][0-9]|[0-9])"
    ))
    .unwrap()
});

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Bash,
    Monitor,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "Bash",
            Self::Monitor => "Monitor",
        }
    }
}

#[derive(Clone)]
struct Use {
    kind: Kind,
    description: String,
    start: f64,
    limit: f64,
}

pub struct Task {
    pub id: String,
    pub kind: Kind,
    pub description: String,
    pub output: Option<String>,
    pub start: f64,
    pub limit: f64,
}

#[derive(Default)]
pub struct Scan {
    pub tasks: Vec<Task>,
    pub ended: HashSet<String>,
    uses: HashMap<String, Use>,
}

pub fn scan(path: &str, now: f64) -> Option<Scan> {
    scan_reader(BufReader::new(File::open(path).ok()?), now).ok()
}

pub fn scan_reader(mut reader: impl BufRead, now: f64) -> io::Result<Scan> {
    let mut scan = Scan::default();
    let mut line = Vec::new();
    while let Some(truncated) = next_line(&mut reader, &mut line)? {
        scan.line(&line, truncated, now);
    }
    Ok(scan)
}

fn next_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Option<bool>> {
    line.clear();
    let mut truncated = false;
    let mut any = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(any.then_some(truncated));
        }
        any = true;
        let end = chunk.iter().position(|b| *b == b'\n');
        let part = &chunk[..end.unwrap_or(chunk.len())];
        let room = LINE_LIMIT.saturating_sub(line.len()).min(part.len());
        line.extend_from_slice(&part[..room]);
        truncated |= room < part.len();
        let used = end.map_or(chunk.len(), |at| at + 1);
        reader.consume(used);
        if end.is_some() {
            return Ok(Some(truncated));
        }
    }
}

pub fn has(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn truthy(value: Option<&Value>) -> Option<&Value> {
    let value = value?;
    let on = match value {
        Value::Null => false,
        Value::Bool(on) => *on,
        Value::Number { raw, .. } => raw.parse::<f64>() != Ok(0.0),
        Value::String(text) => !text.codepoints.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
    };
    on.then_some(value)
}

fn py_str(value: Option<&Value>) -> String {
    match truthy(value) {
        None => String::new(),
        Some(Value::String(text)) => text.lossy().into_owned(),
        Some(Value::Number { output, .. }) => output.clone(),
        Some(Value::Bool(_)) => "True".into(),
        Some(other) => pyjson::dumps(other),
    }
}

fn limit_of(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Bool(on)) => f64::from(u8::from(*on)),
        Some(Value::Number { raw, .. }) => raw.parse().unwrap_or(DEFAULT_MONITOR_MS),
        _ => DEFAULT_MONITOR_MS,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn month_days(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

pub fn epoch(stamp: &str, now: f64) -> f64 {
    let head: String = stamp.chars().take(19).collect();
    let Some(found) = STAMP.captures(&head) else {
        return now;
    };
    if found.get(0).map_or(0, |whole| whole.end()) != head.len() {
        return now;
    }
    let field = |at: usize| found[at].trim().parse::<i64>().unwrap_or(0);
    let (year, month, day) = (field(1), field(2), field(3));
    if year < 1 || day > month_days(year, month) {
        return now;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + field(4) * 3600 + field(5) * 60 + field(6);
    seconds as f64
}

impl Scan {
    fn line(&mut self, raw: &[u8], truncated: bool, now: f64) {
        if has(raw, b"task-id>") {
            let text = String::from_utf8_lossy(raw);
            for found in NOTICE.captures_iter(&text) {
                self.ended.insert(found[1].to_owned());
            }
        }
        if truncated || (!has(raw, b"\"tool_use") && !has(raw, b"toolUseResult")) {
            return;
        }
        let Ok(entry) = pyjson::loads(raw) else {
            return;
        };
        let Some(blocks) = entry
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => self.tool_use(block, &entry, now),
                Some("tool_result") => self.tool_result(block, &entry),
                _ => {}
            }
        }
    }

    fn tool_use(&mut self, block: &Value, entry: &Value, now: f64) {
        let input = block.get("input");
        let name = block.get("name").and_then(Value::as_str);
        if name == Some("TaskStop") {
            let id = truthy(input.and_then(|i| i.get("task_id")))
                .or_else(|| truthy(input.and_then(|i| i.get("shell_id"))));
            if let Some(id) = id.and_then(Value::as_str) {
                self.ended.insert(id.to_owned());
            }
            return;
        }
        let kind = match name {
            Some("Bash") => Kind::Bash,
            Some("Monitor") => Kind::Monitor,
            _ => return,
        };
        let Some(id) = block.get("id").and_then(Value::as_str) else {
            return;
        };
        let stamp = match truthy(entry.get("timestamp")) {
            Some(value @ Value::String(_)) => value.as_str().unwrap_or("").to_owned(),
            _ => String::new(),
        };
        let description: String = py_str(input.and_then(|i| i.get("description"))).chars().take(80).collect();
        self.uses.insert(
            id.to_owned(),
            Use {
                kind,
                description,
                start: epoch(&stamp, now),
                limit: limit_of(input.and_then(|i| i.get("timeout_ms"))),
            },
        );
    }

    fn tool_result(&mut self, block: &Value, entry: &Value) {
        let Some(used) = block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .and_then(|id| self.uses.get(id))
            .cloned()
        else {
            return;
        };
        let result = entry.get("toolUseResult");
        let body = text_of(block.get("content"));
        let direct = truthy(result.and_then(|r| r.get("backgroundTaskId"))).or_else(|| {
            (used.kind == Kind::Monitor)
                .then(|| truthy(result.and_then(|r| r.get("taskId"))))
                .flatten()
        });
        let id = match direct {
            Some(value) => value.as_str().map(str::to_owned),
            None if body.contains("in background with ID") || body.contains("Monitor started") => {
                ID.captures(&body).map(|found| found[1].to_owned())
            }
            None => None,
        };
        let Some(id) = id else {
            return;
        };
        let task = Task {
            output: OUTPUT.captures(&body).map(|found| found[1].to_owned()),
            id,
            kind: used.kind,
            description: used.description,
            start: used.start,
            limit: used.limit,
        };
        match self.tasks.iter_mut().find(|existing| existing.id == task.id) {
            Some(slot) => *slot = task,
            None => self.tasks.push(task),
        }
    }
}

fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.lossy().into_owned(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(|text| text.as_string()))
            .map(|text| text.lossy())
            .collect(),
        _ => String::new(),
    }
}
