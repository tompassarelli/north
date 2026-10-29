use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::common::{redact, state_dir, SQLITE_BUSY};
use crate::platform::sqlite::{self, Access};
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const USAGE: &str = r#"distill: distill Tom's decisions into a long-term ledger that does not decay.

Each run reads what is new since its watermark and appends one JSON row per record to
~/.local/state/north/grounding/ledger.jsonl:

  - Tom's own typed messages in top-level sessions he started (a session whose first prompt is
    an agent brief, or that has a single prompt, is an agent's; brief-shaped prompts and
    questions about what already happened inside Tom's sessions are skipped; requests count as
    directions), kept when deterministic rules read them as a
    decision, direction, preference, correction or approval;
  - DECIDED and APPROVED lines the proxy wrote in Tom's sessions;
  - DECIDED and APPROVED lines in handoff status files.

Sources are convo's SQLite index (read-only) and *status*.md files under the handoffs directory.
Extraction is rules only; no model is called. A newer Tom correction, decision or preference
with an override marker supersedes the latest older Tom decision, direction or preference from
the three days before it that names at least three of the same specific subjects; both rows
are kept.

Each row is {id, at, kind, topic, quote, source, superseded_by}; source starts with who said it
(`tom`, `proxy` or the status file's lead) and names the convo message and session or the file
and line.

  north grounding distill run [--since 14d] [--no-refresh] [--if-idle]
  north grounding distill status
"#;

const MINUTE: f64 = 60_000.0;
const DAY: f64 = 24.0 * 60.0 * MINUTE;
const OVERLAP: f64 = 30.0 * MINUTE;
const SUPERSEDE_WINDOW: f64 = 3.0 * DAY;
const QUOTE_MAX: usize = 500;
const GENERIC: &[&str] = &[
    "issue", "policy", "skill", "hook", "lead", "worker", "test", "handoff", "landing", "review", "memory", "cpu",
    "terminal", "proxy",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Decision,
    Direction,
    Preference,
    Correction,
    Approval,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Direction => "direction",
            Kind::Preference => "preference",
            Kind::Correction => "correction",
            Kind::Approval => "approval",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub id: String,
    pub at: String,
    pub kind: String,
    pub topic: String,
    pub quote: String,
    pub source: String,
    pub superseded_by: Option<String>,
}

struct Paths {
    db: PathBuf,
    state: PathBuf,
    handoffs: PathBuf,
    ledger: PathBuf,
    watermark: PathBuf,
}

fn paths() -> Paths {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let state = state_dir();
    Paths {
        db: env::var_os("NORTH_GROUNDING_CONVO_DB").map(PathBuf::from).unwrap_or_else(|| home.join(".local/state/convo/index-v4.db")),
        handoffs: env::var_os("NORTH_GROUNDING_HANDOFFS").map(PathBuf::from).unwrap_or_else(|| home.join(".local/state/agents/handoffs")),
        ledger: env::var_os("NORTH_GROUNDING_LEDGER").map(PathBuf::from).unwrap_or_else(|| state.join("ledger.jsonl")),
        watermark: state.join("distill-state.json"),
        state,
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("distill pattern")
}

macro_rules! pattern {
    ($name:ident, $text:expr) => {
        fn $name() -> &'static Regex {
            static RE: OnceLock<Regex> = OnceLock::new();
            RE.get_or_init(|| re($text.as_ref()))
        }
    };
}

const B: &str = r"(?-u:\b)";
const W: &str = "[^A-Za-z0-9_]";
const S: &str = r"[\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";

pattern!(brief_session, format!(
    r"(?i)^{S}*(Delegation:|Category:|Parent:|You are (the|a|an) [^\n]{{0,60}}{B}(lead|worker|agent|reviewer){B})"
));
pattern!(brief_read, format!(r"(?i)^{S}*Read [^\s]*handoffs/([^\s]+) and (follow|take over)"));
pattern!(skip_message, format!(
    r"(?i)^{S}*(/[a-z-]+{S}*$|/compact|Delegation:|Category:|Parent:|Caveat:|\[Request interrupted|This session is being continued|Base directory for this skill|Read [^\s]*handoffs/[^\s]*brief)"
));
pattern!(pasted, r"(?s)<pasted_content[^>]*>.*?(</pasted_content>|\z)");
pattern!(line_record, format!(r"^{S}*(?:[-*]{S}+)?(DECIDED|APPROVED){B}[:{}]*([^\n\r\u{{2028}}\u{{2029}}]*)$", &S[1..S.len() - 1]));
pattern!(status_time, format!(r"^([0-9]{{1,2}}):([0-9]{{2}})Z?{B}"));
pattern!(about_keep, format!(r"(?i)^{W}*why (did|do|does|would|are|were|is) (you|u){B}"));
pattern!(about_past, format!(
    r"(?i)^{W}*(why'?d (you|u){B}|how come|what happened|what did (you|u){B}|did (you|u){B})"
));
pattern!(request, format!(r"(?i)^{W}*((can|could|would|will) (you|we|u){B}|let'?s{B}|please{B}|why don'?t (we|you|u){B})"));
pattern!(correction, format!(
    r"(?i){B}(we already (have|did|talked)|i (already )?told you|i'?ve already|i have already|didn'?t i (ask|say|tell)|you keep{B}|that'?s (wrong|not (it|right|what))|not what i (asked|said|meant|want)|why didn'?t you|we did this (recently|already|before)|do we not have|why do we keep|you'?re approaching|i said{B}|as i said|stop (doing|asking|saying)|you were wrong|no,? (that|it)'?s not)"
));
pattern!(decision, format!(
    r"(?i){B}(we('ll| will)? go with|let'?s go with|i'?ve decided|i decided|we decided|decision is|it'?s fine if|its fine if|from now on|durably|new rule|rule change|going forward|instead of|i'?m choosing|we'?re (switching|moving) to)"
));
pattern!(preference, format!(
    r"(?i){B}(i prefer|i'?d prefer|i'?d rather|i don'?t want|i dont want|i want (you|it|them|this|everything|all|every)|only give me|i like (it|when|the)|i hate|i never want|always (use|give|check|answer|respond)|never (use|give|ask|answer|start))"
));
pattern!(direction, format!(
    r"(?i){B}(we need to|we have to|we need (a|an)|need you to|make sure|get it done|be ambitious|focus on|prioriti[sz]e|should (be|have|always|never|only|just)|needs to be|must{B}|keep (it|them|the)|do not{B}|don'?t (ever|let|touch|use|start|close)|remove (the|these|this|that)|close (my|the|all)|burn (it|codex|claude|down))"
));
pattern!(approval, format!(
    r"(?i)^{S}*(approved?|yes|yep|yeah|go ahead|do it|do (all|both|everything)|ship it|land it|sounds good|lgtm|proceed|go for it|let'?s do (it|that)){B}[\t\n\x0B\x0C\r \u{{a0}}\u{{1680}}\u{{2000}}-\u{{200a}}\u{{2028}}\u{{2029}}\u{{202f}}\u{{205f}}\u{{3000}}\u{{feff}}.!,]*([^\n?]{{0,80}})$"
));
pattern!(approved_start, format!(r"(?i)^{S}*approved?{B}"));
pattern!(asked, format!(r"(?i)\?|{B}(approve|approval|should i|want me to|ok to|shall i|your call|confirm|choose|options?){B}"));
pattern!(override_marker, format!(
    r"(?i){B}(instead|actually|rather|prefer|no longer|not anymore|stop|don'?t|change|reverse|scratch that|forget|wrong){B}"
));
pattern!(word_re, "[a-z0-9][a-z0-9'-]*[a-z0-9]|[a-z0-9]");
pattern!(session_re, format!(r"{B}session:([^\s]+)"));
pattern!(since_re, "^([0-9]+)([hdw])$");

fn rules() -> [(Kind, &'static Regex); 4] {
    [(Kind::Correction, correction()), (Kind::Decision, decision()), (Kind::Preference, preference()), (Kind::Direction, direction())]
}

fn is_brief_session(text: &str) -> bool {
    brief_session().is_match(text)
        || brief_read().captures(text).is_some_and(|c| !c[1].to_lowercase().contains("proxy"))
}

fn is_skip_message(text: &str) -> bool {
    let rest = text.trim_start_matches(js_space);
    (rest.starts_with('<') && !rest[1..].to_lowercase().starts_with("pasted_content")) || skip_message().is_match(text)
}

fn about_the_past(text: &str) -> bool {
    about_keep()
        .find(text)
        .is_some_and(|m| !text[m.end()..].get(..5).is_some_and(|next| next.eq_ignore_ascii_case(" keep")))
        || about_past().is_match(text)
}

const SUBJECT_TABLE: &[(&str, &str)] = &[
    ("codex", "codex"), ("sol", "codex"), ("gpt", "codex"), ("openai", "codex"),
    ("claude", "claude"), ("anthropic", "anthropic"), ("opus", "opus"), ("sonnet", "sonnet"), ("haiku", "haiku"),
    ("nexus", "nexus"), ("laptop", "laptop"), ("cloud", "cloud"), ("farm", "farm"),
    ("smashcraft", "smashcraft"), ("wisp", "wisp"), ("wc3", "wc3"), ("warcraft", "wc3"), ("bnet", "bnet"),
    ("north", "north"), ("south", "south"), ("firn", "firn"), ("nixos", "firn"), ("galileo", "galileo"),
    ("telemetry", "telemetry"), ("proton", "proton"), ("convo", "convo"),
    ("proxy", "proxy"), ("lead", "lead"), ("leads", "lead"), ("worker", "worker"), ("workers", "worker"),
    ("usage", "usage"), ("burn", "usage"), ("quota", "usage"), ("pace", "pace"), ("account", "account"),
    ("accounts", "account"), ("cpu", "cpu"), ("load", "cpu"), ("landing", "landing"), ("land", "landing"),
    ("autoland", "landing"), ("safe-push", "landing"), ("review", "review"), ("skill", "skill"), ("skills", "skill"),
    ("hook", "hook"), ("hooks", "hook"), ("rule", "policy"), ("rules", "policy"), ("policy", "policy"),
    ("netcode", "netcode"), ("rollback", "netcode"), ("latency", "latency"), ("test", "test"), ("tests", "test"),
    ("issue", "issue"), ("issues", "issue"), ("threads", "issue"), ("transcripts", "memory"), ("memory", "memory"),
    ("retention", "memory"), ("handoff", "handoff"), ("handoffs", "handoff"), ("timezone", "timezone"),
    ("ghostty", "terminal"), ("terminal", "terminal"), ("terminals", "terminal"),
];

const STOP: &str = "a about above after again against all also am an and any are aren't as at be because been before being
below between both but by can can't cannot could couldn't did didn't do does doesn't doing don't down during each
few for from further get got had hasn't have haven't having he her here hers him his how i i'd i'll i'm i've if in
into is isn't it it's its itself just let let's like make me more most much my myself need needs no nor not now of
off on once only or other our ours out over own really right same shall she should shouldn't so some such than that
that's the their them then there these they this those through to too under until up us very was wasn't way we
we're were weren't what when where which while who whom why will with won't would wouldn't yeah yes you you're your
yours okay ok gonna want think know thing things kind basically actually something someone even still lot im dont
thats youre its isnt cant wont didnt doesnt ive whats theres";

fn subject_of(word: &str) -> Option<&'static str> {
    SUBJECT_TABLE.iter().find(|(w, _)| *w == word).map(|(_, s)| *s)
}

fn stop_words() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| STOP.split_whitespace().collect())
}

fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn squash(s: &str) -> String {
    s.split(js_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn utf16_head(s: &str, units: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        used += c.len_utf16();
        if used > units {
            break;
        }
        out.push(c);
    }
    out
}

fn utf16_tail(s: &str, units: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices().rev() {
        used += c.len_utf16();
        if used > units {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}


pub fn parse_since(s: &str, now: f64) -> Option<f64> {
    if let Some(c) = since_re().captures(s) {
        let n: f64 = c[1].parse().ok()?;
        let unit = match &c[2] {
            "h" => 3_600_000.0,
            "d" => DAY,
            _ => 7.0 * DAY,
        };
        return Some(now - n * unit);
    }
    if s.contains('T') { Timestamp::parse_loose(s).map(|t| t.unix_millis() as f64) } else { Timestamp::parse_loose(&format!("{s}T00:00:00Z")).map(|t| t.unix_millis() as f64) }
}

fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

fn rid(key: &str) -> String {
    format!("L{}", &sha1_hex(key.as_bytes())[..10])
}

fn words(text: &str) -> Vec<String> {
    word_re().find_iter(&text.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

pub fn subjects(text: &str) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for w in words(text) {
        if let Some(s) = subject_of(&w)
            && !seen.contains(&s) {
                seen.push(s);
            }
    }
    seen
}

pub fn topic(text: &str) -> String {
    let found = subjects(text);
    let lower = text.to_lowercase();
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for w in words(text) {
        if w.chars().count() >= 4 && !stop_words().contains(w.as_str()) && subject_of(&w).is_none() && !w.chars().all(|c| c.is_ascii_digit()) {
            let n = counts.entry(w.clone()).or_insert(0);
            if *n == 0 {
                order.push(w);
            }
            *n += 1;
        }
    }
    order.sort_by(|a, b| counts[b].cmp(&counts[a]).then(lower.find(a.as_str()).cmp(&lower.find(b.as_str()))));
    found.iter().map(|s| s.to_string()).chain(order).take(6).collect::<Vec<_>>().join(" ")
}

fn typed_text(text: &str) -> String {
    squash(&pasted().replace_all(text, " "))
}

fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<char> = None;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if js_space(c) && matches!(prev, Some('.' | '!' | '?')) {
            out.push(&text[start..i]);
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = iter.peek() {
                if !js_space(d) {
                    break;
                }
                end = j + d.len_utf8();
                iter.next();
            }
            start = end;
            prev = text[..end].chars().next_back();
            continue;
        }
        prev = Some(c);
    }
    out.push(&text[start..]);
    out
}

pub fn trim_quote(raw: &str, pattern: Option<&Regex>) -> String {
    let text = redact(raw);
    if utf16_len(&text) <= QUOTE_MAX {
        return text;
    }
    let all = sentences(&text);
    let picked: Vec<&str> = all.iter().copied().filter(|s| pattern.is_some_and(|p| p.is_match(s))).collect();
    let chosen = if picked.is_empty() { all[..1].to_vec() } else { picked };
    let mut out = String::new();
    for s in chosen {
        let piece = if out.is_empty() { s.to_string() } else { format!(" … {s}") };
        if utf16_len(&out) + utf16_len(&piece) > QUOTE_MAX {
            break;
        }
        out.push_str(&piece);
    }
    if out.is_empty() { utf16_head(&text, QUOTE_MAX) } else { out }
}

fn counts(s: &str) -> bool {
    !about_the_past(s) && (!s.trim_end_matches(js_space).ends_with('?') || request().is_match(s) || correction().is_match(s))
}

pub fn classify(text: &str, previous_assistant: Option<&str>) -> (Option<Kind>, Option<&'static Regex>) {
    if approval().is_match(text)
        && utf16_len(text) <= 120
        && previous_assistant.is_some_and(|p| !p.is_empty() && asked().is_match(utf16_tail(p, 600)))
    {
        return (Some(Kind::Approval), None);
    }
    if approved_start().is_match(text) {
        return (Some(Kind::Approval), None);
    }
    let statements: Vec<&str> = sentences(text).into_iter().filter(|s| counts(s)).collect();
    for (kind, pattern) in rules() {
        if statements.iter().any(|s| pattern.is_match(s)) {
            return (Some(kind), Some(pattern));
        }
    }
    if statements.iter().any(|s| request().is_match(s)) {
        return (Some(Kind::Direction), Some(request()));
    }
    (None, None)
}

fn is_tom_session(first: Option<&(String, i64)>) -> bool {
    first.is_some_and(|(text, prompts)| *prompts >= 2 && !is_brief_session(text))
}

fn refresh_index() {
    let Ok(mut child) = Command::new("north").args(["convo", "index"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
        }
    }
}

struct Message {
    msg: i64,
    path: String,
    line: i64,
    ts: String,
    role: String,
    text: String,
    session: String,
    previous: Option<String>,
}

fn sql_text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn int_of(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| v.as_f64().map_or(0, |f| f as i64))
}

fn id_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

fn read_convo(db: &Path, since: f64) -> NorthResult<Vec<Message>> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let rows = sqlite::rows(db, Access::ReadOnly, SQLITE_BUSY, &format!(
        "SELECT m.id, m.file_id, m.line, m.ts, m.role, b.text, f.session_id, f.path FROM msg m JOIN files f ON f.id = m.file_id \
         JOIN body b ON b.id = m.body WHERE m.ts > {} AND m.dup = 0 AND f.agent IS NULL AND m.tool IS NULL \
         AND (m.role = 'user' OR (m.role = 'assistant' AND (b.text LIKE '%DECIDED%' OR b.text LIKE '%APPROVED%'))) \
         ORDER BY m.ts, m.id",
        sql_text(&Timestamp::from_unix_millis_f64(since).iso_millis())
    ))?;
    let mut file_ids: Vec<i64> = rows.iter().map(|r| int_of(&r["file_id"])).collect();
    file_ids.sort_unstable();
    file_ids.dedup();
    let mut first: HashMap<i64, (String, i64)> = HashMap::new();
    for chunk in file_ids.chunks(500) {
        let got = sqlite::rows(db, Access::ReadOnly, SQLITE_BUSY, &format!(
            "SELECT u.file_id, b.text, u.n FROM (SELECT file_id, min(id) AS first, count(*) AS n FROM msg \
             WHERE role = 'user' AND file_id IN ({}) GROUP BY file_id) u \
             JOIN msg m ON m.id = u.first JOIN body b ON b.id = m.body",
            id_list(chunk)
        ))?;
        for g in got {
            first.insert(int_of(&g["file_id"]), (text_of(&g["text"]), int_of(&g["n"])));
        }
    }
    let tom: HashSet<i64> = file_ids.iter().copied().filter(|f| is_tom_session(first.get(f))).collect();
    let asking: Vec<i64> = rows
        .iter()
        .filter(|r| tom.contains(&int_of(&r["file_id"])) && r["role"] == "user")
        .map(|r| int_of(&r["id"]))
        .collect();
    let mut previous: HashMap<i64, String> = HashMap::new();
    for chunk in asking.chunks(500) {
        let got = sqlite::rows(db, Access::ReadOnly, SQLITE_BUSY, &format!(
            "SELECT c.id, (SELECT b.text FROM msg m JOIN body b ON b.id = m.body WHERE m.file_id = c.file_id AND m.line < c.line \
             AND m.role = 'assistant' ORDER BY m.line DESC LIMIT 1) AS prev FROM msg c WHERE c.id IN ({})",
            id_list(chunk)
        ))?;
        for g in got {
            if !g["prev"].is_null() {
                previous.insert(int_of(&g["id"]), text_of(&g["prev"]));
            }
        }
    }
    Ok(rows
        .into_iter()
        .filter(|r| tom.contains(&int_of(&r["file_id"])))
        .map(|r| {
            let msg = int_of(&r["id"]);
            let role = text_of(&r["role"]);
            Message {
                previous: if role == "user" { previous.remove(&msg) } else { None },
                msg,
                path: text_of(&r["path"]),
                line: int_of(&r["line"]),
                ts: text_of(&r["ts"]),
                role,
                text: text_of(&r["text"]),
                session: text_of(&r["session_id"]),
            }
        })
        .collect())
}

fn row(id: String, at: String, kind: Kind, text_for_topic: &str, quote: String, source: String) -> Row {
    Row { id, at, kind: kind.name().into(), topic: topic(text_for_topic), quote, source, superseded_by: None }
}

pub fn speaker(r: &Row) -> &str {
    r.source.split(' ').next().unwrap_or("")
}

fn session_of(r: &Row) -> Option<String> {
    session_re().captures(&r.source).map(|c| c[1].to_string())
}

fn line_records(text: &str) -> impl Iterator<Item = (usize, &str, regex::Captures<'_>)> {
    text.split('\n').enumerate().filter_map(|(n, ln)| line_record().captures(ln).map(|c| (n, ln, c)))
}

fn convo_records(messages: &[Message]) -> Vec<Row> {
    let mut out = Vec::new();
    for m in messages {
        let place = format!("convo:{} session:{}", m.msg, m.session);
        let at = Timestamp::parse_loose(&m.ts).unwrap_or_default().iso();
        let key = format!("convo:{}:{}:{}", m.session, m.path, m.line);
        if m.role == "assistant" {
            for (n, ln, hit) in line_records(&m.text) {
                let kind = if &hit[1] == "APPROVED" { Kind::Approval } else { Kind::Decision };
                out.push(row(rid(&format!("{key}:{n}")), at.clone(), kind, hit[2].trim_matches(js_space), ln.trim_matches(js_space).into(), format!("proxy {place}")));
            }
            continue;
        }
        if is_skip_message(&m.text) {
            continue;
        }
        let text = typed_text(&m.text);
        if utf16_len(&text) < 6 {
            continue;
        }
        let (Some(kind), pattern) = classify(&text, m.previous.as_deref()) else { continue };
        let asked = match (&m.previous, kind) {
            (Some(p), Kind::Approval) if !p.is_empty() => utf16_tail(p, 600),
            _ => "",
        };
        out.push(row(rid(&key), at.clone(), kind, &format!("{text} {asked}"), trim_quote(&text, pattern), format!("tom {place}")));
    }
    out
}

fn mtime_ms(meta: &fs::Metadata) -> NorthResult<f64> {
    Ok(Timestamp::from(meta.modified()?).unix_seconds_f64() * 1000.0)
}

fn status_records(handoffs: &Path, since: f64) -> Vec<Row> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(handoffs) else { return out };
    static NAME: OnceLock<Regex> = OnceLock::new();
    let name_re = NAME.get_or_init(|| re(r"status.*\.md$"));
    let mut names: Vec<String> = entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| name_re.is_match(n)).collect();
    names.sort();
    for name in names {
        let path = handoffs.join(&name);
        let Ok(mtime) = fs::metadata(&path).map_err(NorthError::from).and_then(|m| mtime_ms(&m)) else { continue };
        if mtime <= since {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let content = String::from_utf8_lossy(&bytes);
        let lead = name.strip_suffix(".md").unwrap_or(&name).replacen("-status", "", 1);
        for (i, ln, hit) in line_records(&content) {
            let body = hit[2].trim_matches(js_space);
            let mut at = mtime;
            if let Some(clock) = status_time().captures(body) {
                let (h, m): (f64, f64) = (clock[1].parse().unwrap_or(99.0), clock[2].parse().unwrap_or(0.0));
                if h < 24.0 {
                    let day = (mtime.trunc() / DAY).floor() * DAY;
                    let t = day + h * 3_600_000.0 + m * MINUTE;
                    at = if t > mtime { t - DAY } else { t };
                }
            }
            let quote = squash(ln);
            let key = format!("status:{name}:{}", sha1_hex(quote.as_bytes()));
            let kind = if &hit[1] == "APPROVED" { Kind::Approval } else { Kind::Decision };
            out.push(row(rid(&key), Timestamp::from_unix_millis_f64(at).iso(), kind, body, trim_quote(&quote, None), format!("{lead} status:{}:{}", path.display(), i + 1)));
        }
    }
    out
}

pub fn link_supersession(rows: &mut [Row], new_ids: &HashSet<String>) -> usize {
    let mut tom: Vec<usize> = (0..rows.len()).filter(|&i| speaker(&rows[i]) == "tom").collect();
    tom.sort_by(|&a, &b| rows[a].at.cmp(&rows[b].at));
    let mut links = 0;
    for (pos, &newer) in tom.iter().enumerate() {
        let n = &rows[newer];
        if !new_ids.contains(&n.id) || !["correction", "decision", "preference"].contains(&n.kind.as_str()) {
            continue;
        }
        if !override_marker().is_match(&n.quote) {
            continue;
        }
        let mine: HashSet<&str> = subjects(&n.quote).into_iter().filter(|s| !GENERIC.contains(s)).collect();
        let newer_at = Timestamp::parse_loose(&n.at).map(|t| t.unix_millis() as f64).unwrap_or(f64::NAN);
        let (newer_id, newer_quote) = (n.id.clone(), n.quote.clone());
        for &older in tom[..pos].iter().rev() {
            let o = &rows[older];
            if newer_at - Timestamp::parse_loose(&o.at).map(|t| t.unix_millis() as f64).unwrap_or(f64::NAN) > SUPERSEDE_WINDOW {
                break;
            }
            if o.superseded_by.is_some() || !["decision", "direction", "preference"].contains(&o.kind.as_str()) {
                continue;
            }
            if o.quote != newer_quote && subjects(&o.quote).iter().filter(|s| mine.contains(*s)).count() >= 3 {
                rows[older].superseded_by = Some(newer_id);
                links += 1;
                break;
            }
        }
    }
    links
}

fn load_ledger(ledger: &Path) -> NorthResult<Vec<Row>> {
    if !ledger.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(ledger)?;
    text.split('\n').filter(|l| !l.trim().is_empty()).map(|l| Ok(serde_json::from_str(l)?)).collect()
}

fn write_ledger(ledger: &Path, rows: &[Row]) -> NorthResult<()> {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by_key(|a| a.at.clone() + &a.id);
    let mut text = String::new();
    for r in sorted {
        text.push_str(&serde_json::to_string(r)?);
        text.push('\n');
    }
    let tmp = PathBuf::from(format!("{}.tmp", ledger.display()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, ledger)?;
    Ok(())
}

fn lock_age_ms(lock: &Path) -> Option<f64> {
    let meta = fs::metadata(lock).ok()?;
    Some(Timestamp::now().unix_millis() as f64 - mtime_ms(&meta).ok()?)
}

fn with_lock<T>(state: &Path, body: impl FnOnce() -> NorthResult<T>) -> NorthResult<T> {
    let lock = state.join("distill.lock.d");
    let mut tries = 0;
    loop {
        if fs::create_dir(&lock).is_ok() {
            break;
        }
        match lock_age_ms(&lock) {
            Some(age) if age > 10.0 * MINUTE => {
                let _ = fs::remove_dir(&lock);
            }
            _ if tries > 600 => return Err(NorthError::State(format!("distill: another run holds {}", lock.display()))),
            _ => thread::sleep(Duration::from_secs(1)),
        }
        tries += 1;
    }
    let result = body();
    let _ = fs::remove_dir(&lock);
    result
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn read_watermark(path: &Path) -> NorthResult<Value> {
    Ok(if path.exists() { serde_json::from_str(&fs::read_to_string(path)?)? } else { Value::Object(Default::default()) })
}

fn run(p: &Paths, since_arg: Option<&str>, no_refresh: bool, if_idle: bool) -> NorthResult<Result<String, String>> {
    fs::create_dir_all(&p.state)?;
    let lock = p.state.join("distill.lock.d");
    if if_idle && lock.exists() && lock_age_ms(&lock).is_some_and(|age| age <= 10.0 * MINUTE) {
        return Ok(Err(format!("distill: busy, another run holds {}", lock.display())));
    }
    with_lock(&p.state, || {
        let state = read_watermark(&p.watermark)?;
        let now = Timestamp::now().unix_millis() as f64;
        let since = match (since_arg, state.get("through").and_then(Value::as_str)) {
            (Some(s), _) => parse_since(s, now),
            (None, Some(through)) => Timestamp::parse_loose(through).map(|t| t.unix_millis() as f64).map(|t| t - OVERLAP),
            (None, None) => parse_since("14d", now),
        }
        .ok_or_else(|| NorthError::Usage("distill: invalid time value".into()))?;
        let started = Timestamp::now().unix_millis() as f64;
        if !no_refresh {
            refresh_index();
        }
        let mut rows = load_ledger(&p.ledger)?;
        let known: HashSet<String> = rows.iter().map(|r| r.id.clone()).collect();
        let mut said: HashSet<(Option<String>, String)> =
            rows.iter().filter(|r| speaker(r) == "tom").map(|r| (session_of(r), r.quote.clone())).collect();
        let mut unique: Vec<Row> = Vec::new();
        let mut unique_ids: HashSet<String> = HashSet::new();
        let found = convo_records(&read_convo(&p.db, since)?).into_iter().chain(status_records(&p.handoffs, since));
        for r in found {
            let repeat = (session_of(&r), r.quote.clone());
            if known.contains(&r.id) || unique_ids.contains(&r.id) || (speaker(&r) == "tom" && said.contains(&repeat)) {
                continue;
            }
            said.insert(repeat);
            unique_ids.insert(r.id.clone());
            unique.push(r);
        }
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for r in &unique {
            match kinds.iter_mut().find(|(k, _)| *k == r.kind) {
                Some((_, n)) => *n += 1,
                None => kinds.push((r.kind.clone(), 1)),
            }
        }
        let added = unique.len();
        rows.extend(unique);
        let links = link_supersession(&mut rows, &unique_ids);
        write_ledger(&p.ledger, &rows)?;
        fs::write(&p.watermark, format!("{{\"through\":{},\"rows\":{}}}\n", json_str(&Timestamp::from_unix_millis_f64(started).iso()), rows.len()))?;
        let kinds = kinds.iter().map(|(k, n)| format!("{}:{n}", json_str(k))).collect::<Vec<_>>().join(",");
        Ok(Ok(format!(
            "{{\"added\":{added},\"kinds\":{{{kinds}}},\"superseded\":{links},\"total\":{},\"since\":{},\"ledger\":{}}}",
            rows.len(),
            json_str(&Timestamp::from_unix_millis_f64(since).iso()),
            json_str(&p.ledger.display().to_string())
        )))
    })
}

fn status() -> NorthResult<i32> {
    let p = paths();
    let rows = load_ledger(&p.ledger)?;
    let state = read_watermark(&p.watermark)?;
    let through = state.get("through").cloned().unwrap_or(Value::Null);
    println!(
        "{{\"total\":{},\"superseded\":{},\"through\":{},\"ledger\":{}}}",
        rows.len(),
        rows.iter().filter(|r| r.superseded_by.is_some()).count(),
        through,
        json_str(&p.ledger.display().to_string())
    );
    Ok(0)
}

pub fn main(args: &[String]) -> NorthResult<i32> {
    let rest = args.get(1..).unwrap_or_default();
    let has = |flag: &str| rest.iter().any(|a| a == flag);
    match args.first().map(String::as_str) {
        Some("run") => {
            let since = rest.iter().position(|a| a == "--since").and_then(|i| rest.get(i + 1)).map(String::as_str);
            match run(&paths(), since, has("--no-refresh"), has("--if-idle"))? {
                Ok(line) => println!("{line}"),
                Err(busy) => eprintln!("{busy}"),
            }
            Ok(0)
        }
        Some("status") => status(),
        _ => {
            eprintln!("{USAGE}");
            Ok(2)
        }
    }
}

#[cfg(test)]
#[path = "distill_tests.rs"]
mod tests;
