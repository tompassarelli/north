use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::html::PublicPage;
use super::digest;
use crate::foundation::clock::Timestamp;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS watch_snapshot(
  source TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL CHECK(json_valid(value)),
  fetched TEXT NOT NULL, PRIMARY KEY(source,key)
) STRICT;
CREATE TABLE IF NOT EXISTS model_events(
  ts TEXT NOT NULL, model TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('released','expected','market')),
  at TEXT, p REAL CHECK(p IS NULL OR (p >= 0 AND p <= 1)),
  source TEXT NOT NULL, detail TEXT NOT NULL
) STRICT;
";

pub(super) const ATOM: &str = "{http://www.w3.org/2005/Atom}";
pub(super) const PHRASE: &str = r"(?:GPT[ -]?(?:(?:Sol|Astra|Luna|Terra|Image)[ -]+)?\d+(?:\.\d+)*(?:o)?(?:[ -](?:Sol|Astra|Luna|Terra|Codex))?|Claude[ -](?:(?:Opus|Sonnet|Haiku|Fable)[ -])?\d+(?:\.\d+)*|Gemini[ -]\d+(?:\.\d+)*(?:[ -](?:Pro|Flash|Argon))?|Grok[ -]\d+(?:\.\d+)*|DeepSeek[ -][RV]?\d+(?:\.\d+)*)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Row {
    pub(super) model: String,
    pub(super) kind: String,
    pub(super) title: String,
    pub(super) at: Option<String>,
    pub(super) p: Option<f64>,
    pub(super) url: String,
    pub(super) basis: String,
}

#[derive(Clone, Debug)]
pub(super) struct Event {
    pub(super) row: Row,
    pub(super) change: &'static str,
    pub(super) ts: String,
    pub(super) source: String,
    pub(super) id: i64,
}

pub(super) struct Source {
    pub(super) name: String,
    pub(super) parser: String,
    pub(super) endpoint: String,
}

pub(super) type Rows = BTreeMap<String, Row>;

pub(super) fn pattern(cell: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(source).expect("static pattern"))
}

pub(super) fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(1200).collect()
}

pub(super) fn model_names(text: &str) -> Vec<String> {
    static OPENAI: OnceLock<Regex> = OnceLock::new();
    static CLAUDE: OnceLock<Regex> = OnceLock::new();
    static SHORT: OnceLock<Regex> = OnceLock::new();
    let mut result = BTreeSet::new();
    let openai = pattern(&OPENAI, r"(?i)\bgpt[ -]?(?:(sol|astra|luna|terra|image)[ -]+)?(\d+(?:\.\d+)*(?:o)?)(?:[ -](sol|astra|luna|terra|codex))?\b");
    for found in openai.captures_iter(text) {
        let family = found.get(1).or(found.get(3)).map(|family| family.as_str().to_lowercase());
        result.insert(format!("openai:gpt-{}{}", found[2].to_lowercase(), family.map_or_else(String::new, |family| format!("-{family}"))));
    }
    let claude = pattern(&CLAUDE, r"(?i)\bclaude[ -](?:(opus|sonnet|haiku|fable)[ -])?(\d+(?:[.-]\d+)*)\b");
    for found in claude.captures_iter(text) {
        let family = found.get(1).map_or_else(String::new, |family| format!("{}-", family.as_str().to_lowercase()));
        result.insert(format!("anthropic:claude-{family}{}", found[2].replace('.', "-")));
    }
    if text.to_lowercase().contains("anthropic") {
        let short = pattern(&SHORT, r"(?i)\b(opus|sonnet|haiku|fable)[ -](\d+(?:\.\d+)*)\b");
        for found in short.captures_iter(text) {
            result.insert(format!("anthropic:claude-{}-{}", found[1].to_lowercase(), found[2].replace('.', "-")));
        }
    }
    for (provider, family) in [("google", "gemini"), ("xai", "grok"), ("deepseek", "deepseek")] {
        let expression = Regex::new(&format!(r"(?i)\b{family}[ -]((?:[rv])?\d+(?:\.\d+)*)(?:[ -](pro|flash|argon))?\b")).expect("family pattern");
        for found in expression.captures_iter(text) {
            result.insert(format!("{provider}:{family}-{}{}", found[1].to_lowercase(), found.get(2).map_or_else(String::new, |tail| format!("-{}", tail.as_str().to_lowercase()))));
        }
    }
    result.into_iter().collect()
}

pub(super) fn date(value: &Value) -> Option<String> {
    let stamp = match value {
        Value::Number(number) if number.as_f64().is_some_and(f64::is_finite) => Timestamp::from_unix_millis_f64(number.as_f64()?),
        Value::String(text) if !text.is_empty() => date_text(text)?,
        _ => return None,
    };
    Some(stamp.iso())
}

fn date_text(text: &str) -> Option<Timestamp> {
    let iso = text.replace('Z', "+00:00");
    Timestamp::parse(&iso).ok().or_else(|| Timestamp::parse_loose(&iso)).or_else(|| rfc2822(text)).or_else(|| month_day_year(text))
}

fn rfc2822(text: &str) -> Option<Timestamp> {
    static FORM: OnceLock<Regex> = OnceLock::new();
    let form = pattern(&FORM, r"^\s*(?:[A-Za-z]{3},\s*)?(\d{1,2})\s+([A-Za-z]{3})[a-z]*\s+(\d{2,4})\s+(\d{1,2}):(\d{2})(?::(\d{2}))?\s*(?:([+-]\d{4})|(GMT|UTC|UT|Z|EST|EDT|CST|CDT|MST|MDT|PST|PDT))?\s*$");
    let found = form.captures(text)?;
    let month = month_number(&found[2])?;
    let mut year: i64 = found[3].parse().ok()?;
    if found[3].len() <= 2 {
        year += if year <= 68 { 2000 } else { 1900 };
    }
    let offset = match (found.get(7), found.get(8).map(|zone| zone.as_str())) {
        (Some(numeric), _) => format!("{}:{}", &numeric.as_str()[..3], &numeric.as_str()[3..]),
        (None, Some("EST" | "CDT")) => "-05:00".into(),
        (None, Some("EDT")) => "-04:00".into(),
        (None, Some("CST" | "MDT")) => "-06:00".into(),
        (None, Some("MST" | "PDT")) => "-07:00".into(),
        (None, Some("PST")) => "-08:00".into(),
        _ => "+00:00".into(),
    };
    let second = found.get(6).map_or("00", |second| second.as_str());
    Timestamp::parse(&format!("{year:04}-{month:02}-{:02}T{:02}:{}:{second}{offset}", found[1].parse::<u8>().ok()?, found[4].parse::<u8>().ok()?, &found[5])).ok()
}

fn month_number(name: &str) -> Option<u8> {
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|month| name.eq_ignore_ascii_case(month)).map(|index| index as u8 + 1)
}

fn month_day_year(text: &str) -> Option<Timestamp> {
    static FORM: OnceLock<Regex> = OnceLock::new();
    let found = pattern(&FORM, r"^([A-Za-z]{3}) (\d{1,2}), (\d{4})$").captures(text)?;
    Timestamp::parse(&format!("{}-{:02}-{:02}T00:00:00Z", &found[3], month_number(&found[1])?, found[2].parse::<u8>().ok()?)).ok()
}

pub(super) fn probability(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse().ok()?,
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        _ => return None,
    };
    (number.is_finite() && (0.0..=1.0).contains(&number)).then(|| format!("{number:.6}").parse().unwrap_or(number))
}

pub(super) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

pub(super) fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub(super) fn string_field<'a>(market: &'a Value, name: &str) -> Result<&'a str, String> {
    market.get(name).and_then(Value::as_str).ok_or_else(|| format!("{name} required"))
}

pub(super) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if matches!(character, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            lines.push(&text[start..index]);
            start = index + character.len_utf8();
            if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
                characters.next();
                start += 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

pub(super) fn sections(text: &str) -> Vec<(String, String)> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    pattern(&HEADING, r"(?m)^## ").split(text).skip(1).map(|section| {
        let (version, body) = section.split_once('\n').unwrap_or((section, ""));
        (version.to_owned(), body.to_owned())
    }).collect()
}

pub(super) fn diff(previous: &BTreeMap<String, Value>, current: &Rows, threshold: f64) -> Vec<(Row, &'static str)> {
    let mut events = Vec::new();
    for (key, value) in current {
        let wanted = serde_json::to_value(value).unwrap_or(Value::Null);
        let before = previous.get(key);
        if before == Some(&wanted) {
            continue;
        }
        if let Some(before) = before.filter(|_| value.kind == "market") {
            let same = ["model", "kind", "title", "at", "url", "basis"].iter().all(|field| before.get(*field) == wanted.get(*field));
            let moved = before["p"].as_f64().zip(value.p).is_some_and(|(old, new)| (new - old).abs() < threshold);
            if same && moved {
                continue;
            }
        }
        events.push((value.clone(), if before.is_none() { "first-seen" } else { "changed" }));
    }
    events
}

pub(super) fn persist(db: &Connection, source: &str, current: &Rows, fetched: &str, threshold: f64) -> Result<Vec<Event>, String> {
    let sql = |error: rusqlite::Error| error.to_string();
    let previous: BTreeMap<String, Value> = db
        .prepare("SELECT key,value FROM watch_snapshot WHERE source=?")
        .and_then(|mut statement| statement.query_map([source], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>())
        .map_err(sql)?
        .into_iter()
        .map(|(key, value)| (key, serde_json::from_str(&value).unwrap_or(Value::Null)))
        .collect();
    let comparable: BTreeMap<String, Value> = previous.iter().filter(|(key, _)| key.as_str() != "@baseline").map(|(key, value)| (key.clone(), value.clone())).collect();
    let found = if previous.is_empty() { Vec::new() } else { diff(&comparable, current, threshold) };
    let mut events = Vec::new();
    db.execute_batch("BEGIN").map_err(sql)?;
    let outcome = (|| {
        db.execute("DELETE FROM watch_snapshot WHERE source=?", [source])?;
        db.execute("INSERT INTO watch_snapshot VALUES(?,?,?,?)", params![source, "@baseline", "{}", fetched])?;
        for (key, value) in current {
            db.execute("INSERT OR REPLACE INTO watch_snapshot VALUES(?,?,?,?)", params![source, key, serde_json::to_string(value).unwrap_or_default(), fetched])?;
        }
        for (row, change) in found {
            let detail = json!({"title": row.title, "url": row.url, "basis": row.basis, "change": change}).to_string();
            db.execute("INSERT INTO model_events(ts,model,kind,at,p,source,detail) VALUES(?,?,?,?,?,?,?)", params![fetched, row.model, row.kind, row.at, row.p, source, detail])?;
            events.push(Event { row, change, ts: fetched.into(), source: source.into(), id: db.last_insert_rowid() });
        }
        Ok::<(), rusqlite::Error>(())
    })();
    match outcome {
        Ok(()) => db.execute_batch("COMMIT").map_err(sql)?,
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error.to_string());
        }
    }
    Ok(events)
}

pub(super) fn proposal(event: &Event) -> (String, String) {
    let (vendor, name) = event.row.model.split_once(':').unwrap_or(("", &event.row.model));
    let provider = match vendor {
        "openai" => "codex",
        "anthropic" => "claude",
        "google" => "gemini",
        "xai" => "grok",
        other => other,
    };
    let key = format!("{provider}:{name}");
    let fields = [
        ("model", name),
        ("freshness", "new"),
        ("source", event.row.url.as_str()),
        ("observed_at", event.ts.as_str()),
        ("date", event.row.at.as_deref().unwrap_or("unknown")),
        ("basis", event.row.basis.as_str()),
    ];
    let body: Vec<String> = fields.iter().map(|(field, value)| format!("{field} = {}", json!(value))).collect();
    (key.clone(), format!("[models.{}]\n{}\n", json!(key), body.join("\n")))
}

pub(super) fn release_candidates(source: &Source, payload: &[u8], models: &Rows) -> Result<digest::Rows, String> {
    let mut rows = digest::Rows::new();
    match source.parser.as_str() {
        "rss" | "atom" => rows = digest::feed_rows("feed", payload)?,
        "changelog" => {
            let text = std::str::from_utf8(payload).map_err(|_| "payload is not UTF-8".to_owned())?;
            for (version, body) in sections(text) {
                let url = format!("{}#{}", source.endpoint, version.trim());
                let title: String = clean(&format!("Claude Code {version}: {body}")).chars().take(300).collect();
                rows.insert(url.clone(), (title, url));
            }
        }
        "anthropic-news" => {
            let page = PublicPage::parse(std::str::from_utf8(payload).map_err(|_| "payload is not UTF-8".to_owned())?);
            for (href, title) in page.links {
                if href.starts_with("/news/") && !title.trim().is_empty() {
                    let url = format!("https://www.anthropic.com{href}");
                    rows.insert(url.clone(), (clean(&title).chars().take(300).collect(), url));
                }
            }
        }
        _ => {}
    }
    let known: BTreeSet<&str> = models.values().map(|row| row.url.as_str()).collect();
    rows.retain(|_, (_, url)| !known.contains(url.as_str()));
    Ok(rows)
}
