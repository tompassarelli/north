use std::sync::LazyLock;

use regex::Regex;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};
use time::OffsetDateTime;

use super::args::Args;
use super::db::placeholders;
pub(super) use super::hit::Hit;
use super::rank::{Group, rank_sessions};
use super::text::{norm, strip};

pub(super) const CANDIDATES: i64 = 400;
pub(super) const RELAX_BELOW: usize = 3;
pub(super) const ROLES: [&str; 5] = ["user", "assistant", "thinking", "tool", "system"];

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").expect("word expression"));
static TOKENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"]*"?|[()]|[^\s()"]+"#).expect("token expression"));
static PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\w+\*$").expect("prefix expression"));
static RELATIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\s*([hdwmy])$").expect("relative expression"));
static ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d\d-\d\d(?:[T ][\d:.]+Z?)?$").expect("date expression"));

pub(super) struct Plan {
    pub matches: Vec<String>,
    pub partial: Option<String>,
    pub literal: Option<String>,
    pub terms: Vec<String>,
}

pub(super) fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

pub(super) fn query_words(text: &str) -> Vec<String> {
    WORD.find_iter(text).map(|word| word.as_str().to_lowercase()).collect()
}

pub(super) fn plan_query(text: &str, exact: bool, strict: bool) -> Option<Plan> {
    let terms = query_words(text);
    if terms.is_empty() {
        return None;
    }
    if exact {
        let literal = super::text::words(text).join(" ");
        return Some(Plan { matches: vec![quote(&literal)], partial: None, literal: Some(literal), terms });
    }
    let plain = terms.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" ");
    let (mut parts, mut explicit) = (Vec::new(), false);
    for found in TOKENS.find_iter(text) {
        let token = found.as_str();
        if matches!(token, "AND" | "OR" | "NOT" | "(" | ")") {
            explicit = true;
            parts.push(token.to_owned());
        } else if PREFIX.is_match(token) {
            parts.push(format!("{}*", quote(&token[..token.len() - 1])));
        } else if WORD.is_match(token.trim_matches('"')) {
            parts.push(quote(token.trim_matches('"')));
        }
    }
    let mut matches = vec![parts.join(" ")];
    if explicit {
        matches.push(plain);
    }
    let mut distinct: Vec<&String> = Vec::new();
    for term in &terms {
        if !distinct.contains(&term) {
            distinct.push(term);
        }
    }
    let partial = (!(explicit || strict) && distinct.len() > 1).then(|| distinct.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" OR "));
    Some(Plan { matches, partial, literal: None, terms })
}

fn utc(seconds: i64) -> String {
    let moment = OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        moment.year(), u8::from(moment.month()), moment.day(), moment.hour(), moment.minute(), moment.second()
    )
}

/// `12h`, `3d`, `2w`, `6m`, `1y` or an ISO date, as a UTC ISO string.
pub(super) fn parse_when(text: &str, now: i64) -> Result<String, String> {
    let text = strip(text);
    if let Some(found) = RELATIVE.captures(text) {
        let unit = match &found[2] {
            "h" => 3600,
            "d" => 86400,
            "w" => 7 * 86400,
            "m" => 30 * 86400,
            _ => 365 * 86400,
        };
        let count: i64 = found[1].parse().map_err(|_| format!("cannot read time {text:?}: use 12h, 3d, 2w, 6m, 1y or YYYY-MM-DD"))?;
        return Ok(utc(now.saturating_sub(count.saturating_mul(unit))));
    }
    if ISO.is_match(text) {
        return Ok(text.replace(' ', "T"));
    }
    Err(format!("cannot read time {}: use 12h, 3d, 2w, 6m, 1y or YYYY-MM-DD", super::text::repr(text)))
}

const COLUMNS_TAIL: &str = "m.id, m.file_id, m.line, m.ts, m.role, m.tool, m.body, {score} AS bm25, f.session_id, f.agent, f.path, f.project, f.provider";

pub(super) struct Filters {
    pub clauses: Vec<String>,
    pub params: Vec<Value>,
    pub keep_calls: bool,
    pub session: bool,
}

impl Filters {
    pub(super) fn new(args: &Args, now: i64) -> Result<Self, String> {
        let mut filters = Self { clauses: Vec::new(), params: Vec::new(), keep_calls: false, session: false };
        if let Some(role) = &args.role {
            let roles: Vec<&str> = role.split(',').map(strip).filter(|part| !part.is_empty()).collect();
            if let Some(bad) = roles.iter().find(|role| !ROLES.contains(role)) {
                return Err(format!("unknown role {}: use {}", super::text::repr(bad), ROLES.join(", ")));
            }
            filters.add(format!("m.role IN ({})", placeholders(roles.len())), roles.iter().map(|role| Value::Text((*role).to_owned())));
        }
        if let Some(tool) = &args.tool {
            filters.add("m.tool = ? COLLATE NOCASE".to_owned(), [Value::Text(tool.clone())]);
        }
        if let Some(project) = &args.project {
            filters.add("f.project LIKE ?".to_owned(), [Value::Text(format!("%{project}%"))]);
        }
        if let Some(provider) = &args.provider {
            filters.add("f.provider = ?".to_owned(), [Value::Text(if provider == "claude" { "anthropic" } else { "openai" }.to_owned())]);
        }
        if let Some(since) = &args.since {
            filters.add("m.ts >= ?".to_owned(), [Value::Text(parse_when(since, now)?)]);
        }
        if let Some(until) = &args.until {
            filters.add("m.ts < ?".to_owned(), [Value::Text(parse_when(until, now)?)]);
        }
        if let Some(session) = &args.session {
            let prefix = session_prefix(session);
            filters.add(
                "m.file_id IN (SELECT id FROM files WHERE session_id LIKE ? OR agent LIKE ?)".to_owned(),
                [Value::Text(format!("{prefix}%")), Value::Text(format!("{prefix}%"))],
            );
        }
        filters.keep_calls = args.role.is_some() || args.tool.is_some();
        filters.session = args.session.is_some();
        Ok(filters)
    }

    fn add(&mut self, clause: String, params: impl IntoIterator<Item = Value>) {
        self.clauses.push(clause);
        self.params.extend(params);
    }
}

pub(super) fn session_prefix(text: &str) -> String {
    let lower = strip(text).to_lowercase();
    let last = lower.rsplit('/').next().unwrap_or("");
    last.strip_prefix("agent-").unwrap_or(last).to_owned()
}

fn columns(score: &str) -> String {
    COLUMNS_TAIL.replace("{score}", score)
}

fn as_hits(con: &Connection, rows: Vec<(Hit, i64)>) -> rusqlite::Result<Vec<Hit>> {
    let mut texts = std::collections::HashMap::new();
    let mut ids: Vec<i64> = rows.iter().map(|(_, body)| *body).collect();
    ids.sort_unstable();
    ids.dedup();
    for part in ids.chunks(500) {
        let mut statement = con.prepare(&format!("SELECT id, text FROM body WHERE id IN ({})", placeholders(part.len())))?;
        for row in statement.query_map(params_from_iter(part), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))? {
            let (id, text) = row?;
            texts.insert(id, text);
        }
    }
    Ok(rows.into_iter().map(|(hit, body)| Hit { text: texts.get(&body).cloned().unwrap_or_default(), ..hit }).collect())
}

fn hit_row(row: &rusqlite::Row) -> rusqlite::Result<(Hit, i64)> {
    let path: String = row.get(10)?;
    let session: Option<String> = row.get(8)?;
    Ok((
        Hit {
            id: row.get(0)?,
            line: row.get(2)?,
            ts: row.get(3)?,
            role: row.get(4)?,
            tool: row.get(5)?,
            bm25: row.get(7)?,
            session: session.unwrap_or_else(|| path.clone()),
            agent: row.get(9)?,
            path,
            project: row.get(11)?,
            text: String::new(),
            w: 0.0,
        },
        row.get(6)?,
    ))
}

/// Top `k` hits by bm25. A text is indexed at its first occurrence, so with filters every occurrence is searched and the first one that passes is kept.
pub(super) fn fetch(con: &Connection, expr: &str, filters: &Filters, k: i64, literal: Option<&str>) -> rusqlite::Result<Vec<Hit>> {
    let text_of = if filters.clauses.is_empty() { "m.body" } else { "o.body" };
    let mut clauses = vec!["ftx MATCH ?".to_owned()];
    clauses.extend(filters.clauses.iter().cloned());
    if let Some(literal) = literal {
        let literal = literal.to_owned();
        con.create_scalar_function("has_literal", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, move |context| {
            let text: Option<String> = context.get(0)?;
            Ok(norm(&text.unwrap_or_default()).contains(&literal))
        })?;
        clauses.push(format!("has_literal((SELECT text FROM body WHERE id = {text_of}))"));
    }
    let columns = columns("bm25(ftx)");
    let sql = if filters.clauses.is_empty() {
        format!("SELECT {columns} FROM ftx JOIN msg m ON m.id = ftx.rowid JOIN files f ON f.id = m.file_id WHERE {} ORDER BY bm25 LIMIT ?", clauses.join(" AND "))
    } else {
        format!(
            "WITH h AS MATERIALIZED (SELECT {columns} FROM ftx JOIN msg o ON o.id = ftx.rowid JOIN msg m ON m.body = o.body JOIN files f ON f.id = m.file_id WHERE {}) \
             SELECT MIN(id) AS id, file_id, line, ts, role, tool, body, bm25, session_id, agent, path, project, provider FROM h GROUP BY body ORDER BY bm25 LIMIT ?",
            clauses.join(" AND ")
        )
    };
    let mut params = vec![Value::Text(expr.to_owned())];
    params.extend(filters.params.iter().cloned());
    params.push(Value::Integer(k));
    let rows = con.prepare(&sql)?.query_map(params_from_iter(params), hit_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    as_hits(con, rows)
}

pub(super) fn recent(con: &Connection, filters: &Filters, k: i64) -> rusqlite::Result<Vec<Hit>> {
    let mut clauses = Vec::new();
    if !filters.session {
        clauses.push("m.dup = 0".to_owned());
    }
    clauses.extend(filters.clauses.iter().cloned());
    let wher = if clauses.is_empty() { "1".to_owned() } else { clauses.join(" AND ") };
    let sql = format!("SELECT {} FROM msg m JOIN files f ON f.id = m.file_id WHERE {wher} ORDER BY m.ts DESC LIMIT ?", columns("0"));
    let mut params = filters.params.clone();
    params.push(Value::Integer(k));
    let rows = con.prepare(&sql)?.query_map(params_from_iter(params), hit_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    as_hits(con, rows)
}

#[derive(Default)]
pub(super) struct Info {
    pub title: Option<String>,
    pub project: Option<String>,
    pub provider: String,
}

pub(super) fn provider_name(provider: Option<String>) -> String {
    match provider.as_deref() {
        Some("anthropic") => "claude".to_owned(),
        Some("openai") => "codex".to_owned(),
        Some(other) => other.to_owned(),
        None => "?".to_owned(),
    }
}

/// Per session: title, project and provider, preferring the main transcript.
pub(super) fn session_info(con: &Connection, sessions: &[&str]) -> rusqlite::Result<std::collections::HashMap<String, Info>> {
    let mut info: std::collections::HashMap<String, Info> = std::collections::HashMap::new();
    let keys: Vec<&str> = sessions.iter().copied().filter(|key| !key.is_empty()).collect();
    for part in keys.chunks(500) {
        let sql = format!(
            "SELECT session_id, title, project, provider FROM files WHERE session_id IN ({}) ORDER BY agent IS NOT NULL, nmsg DESC",
            placeholders(part.len())
        );
        let mut statement = con.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(part), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;
        for row in rows {
            let (session, title, project, provider) = row?;
            let entry = info.entry(session).or_insert_with(|| Info { title: None, project, provider: provider_name(provider) });
            if entry.title.is_none() {
                entry.title = title.filter(|title| !title.is_empty());
            }
        }
    }
    Ok(info)
}

/// Hits and ranked sessions; widens the candidate set while it is full and every candidate was dropped, so a miss is never an artefact of the limit.
pub(super) fn gather(
    con: &Connection,
    expr: &str,
    filters: &Filters,
    terms: &[String],
    mine: &std::collections::HashSet<String>,
    literal: Option<&str>,
    coverage: bool,
) -> rusqlite::Result<(Vec<Hit>, Vec<Group>)> {
    let mut k = CANDIDATES;
    loop {
        let hits = fetch(con, expr, filters, k, literal)?;
        let groups = rank_sessions(&hits, terms, mine, filters.keep_calls, coverage);
        if !groups.is_empty() || (hits.len() as i64) < k {
            return Ok((hits, groups));
        }
        k *= 8;
    }
}
