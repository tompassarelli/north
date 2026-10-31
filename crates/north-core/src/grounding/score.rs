use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;
use serde_json::Value;

use super::common::{state_dir, SQLITE_BUSY};
use crate::platform::sqlite::{self, Access};
use super::compose::{retrieve, select, LIMIT_CHARS};
use super::store::Hit;
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const CANDIDATES: usize = 500;
const REAL_LIMIT: usize = 80;
const FLOOR: f64 = 0.2;
const CUT_ORDER: [&str; 6] = ["limit", "floor", "dedup", "category cap", "render", "char budget"];
const HISTORY: &str = "'transcripts','builds','ledger','commits','threads','github'";

#[derive(Deserialize)]
struct Needed {
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    tier: Option<String>,
    question: String,
    asked_at: Option<String>,
    session: Option<String>,
    convo_id: Option<i64>,
    needed: Vec<Needed>,
}

#[derive(Deserialize)]
struct MissRow {
    at: Option<String>,
    query: String,
    should_have_found: Vec<String>,
}

struct Row {
    id: String,
    tier: String,
    needed: usize,
    a: usize,
    ab: usize,
    misses_a: Vec<String>,
    misses_ab: Vec<String>,
    chars: usize,
}

struct Miss {
    id: String,
    tier: String,
    reference: String,
    class: char,
    rank: Option<Option<usize>>,
    cut: Option<&'static str>,
    doc: Option<String>,
}

struct Query {
    id: String,
    tier: String,
    question: String,
    refs: Vec<String>,
    block: String,
    as_of: f64,
    prior: String,
}

struct Options {
    with_b: bool,
    runs: usize,
    holdout: Option<String>,
    leaky: bool,
    exclude: Vec<String>,
}

#[derive(Clone)]
enum J {
    Null,
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

fn num_text(n: f64) -> String {
    if !n.is_finite() {
        return "null".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return format!("{}", n as i64);
    }
    serde_json::Number::from_f64(n).map_or("null".into(), |v| v.to_string())
}

impl J {
    fn obj(pairs: Vec<(&str, J)>) -> J {
        J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn strs(items: &[String]) -> J {
        J::Arr(items.iter().cloned().map(J::Str).collect())
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        let pad = |out: &mut String, d: usize| {
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * d));
            }
        };
        match self {
            J::Null => out.push_str("null"),
            J::Num(n) => out.push_str(&num_text(*n)),
            J::Str(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
            J::Arr(items) if items.is_empty() => out.push_str("[]"),
            J::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    pad(out, depth + 1);
                    item.write(out, indent, depth + 1);
                }
                pad(out, depth);
                out.push(']');
            }
            J::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    pad(out, depth + 1);
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push_str(if indent.is_some() { ": " } else { ":" });
                    v.write(out, indent, depth + 1);
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }

    fn text(&self, indent: Option<usize>) -> String {
        let mut out = String::new();
        self.write(&mut out, indent, 0);
        out
    }
}

fn as_of_of(at: Option<&str>, leaky: bool) -> f64 {
    match at.and_then(Timestamp::parse_loose) {
        Some(at) if !leaky => at.unix_millis() as f64 / 1000.0,
        _ => 0.0,
    }
}

fn js_number(n: f64) -> String {
    num_text(n)
}

fn run_query(question: &str, tier: &str, as_of: f64, prior: &str, state: &Path) -> NorthResult<(String, f64)> {
    let started = Instant::now();
    let output = Command::new(env::current_exe()?)
        .args(["grounding", "query", "--tier", tier, "--query", question])
        .env("NORTH_GROUNDING_AS_OF", if as_of != 0.0 { js_number(as_of) } else { String::new() })
        .env("NORTH_GROUNDING_PRIOR", prior)
        .env("NORTH_GROUNDING_STATE", state)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    Ok((String::from_utf8_lossy(&output.stdout).into_owned(), started.elapsed().as_secs_f64()))
}

fn pct(values: &[f64], q: f64) -> J {
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return J::Null;
    }
    let i = ((q * (s.len() - 1) as f64).round() as usize).min(s.len() - 1);
    J::Num(s[i])
}

fn read_rows<T: for<'de> Deserialize<'de>>(path: &Path) -> NorthResult<Vec<T>> {
    let text = fs::read_to_string(path)?;
    text.split('\n').filter(|l| !l.trim().is_empty()).map(|l| Ok(serde_json::from_str(l)?)).collect()
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

fn doc_key(source: &str, id: &str) -> String {
    format!("{source}\u{0}{id}")
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

fn convo_db() -> PathBuf {
    env::var_os("NORTH_GROUNDING_CONVO_DB").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/convo/index-v4.db")
    })
}

fn containing(refs: &HashSet<String>, db: &Path) -> NorthResult<HashMap<String, HashSet<String>>> {
    let mut found = HashMap::new();
    for reference in refs {
        let rows = sqlite::rows(db, Access::ReadOnly, SQLITE_BUSY, &format!(
            "select source, id from docs where instr(ref || char(10) || title || char(10) || text, {}) > 0",
            sql_text(reference)
        ))?;
        found.insert(reference.clone(), rows.iter().map(|r| doc_key(&text_of(&r["source"]), &text_of(&r["id"]))).collect());
    }
    Ok(found)
}

fn bm25_keys(db: &Path, as_of: f64, words: &[String]) -> NorthResult<Vec<String>> {
    let seen = if as_of != 0.0 {
        format!(" and not ((decays = 1 or source in ({HISTORY})) and at > {})", js_number(as_of))
    } else {
        String::new()
    };
    let top = words.iter().take(16).map(|w| format!("\"{}\"", w.replace('"', ""))).collect::<Vec<_>>().join(" OR ");
    let rows = sqlite::rows(db, Access::ReadOnly, SQLITE_BUSY, &format!(
        "select source, id from docs where docs match {}{seen} order by bm25(docs, 0, 0, 0, 4.0, 1.0, 0, 0)",
        sql_text(&top)
    ))?;
    Ok(rows.iter().map(|r| doc_key(&text_of(&r["source"]), &text_of(&r["id"]))).collect())
}

fn hit_key(hit: &Hit) -> String {
    doc_key(&hit.source, &hit.id)
}

fn diagnose(queries: &[Query], db: &Path, now: f64) -> NorthResult<Vec<Miss>> {
    let missed: Vec<(&Query, Vec<&String>)> = queries
        .iter()
        .map(|q| (q, q.refs.iter().filter(|r| !q.block.contains(r.as_str())).collect::<Vec<_>>()))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    let refs: HashSet<String> = missed.iter().flat_map(|(_, m)| m.iter().map(|r| r.to_string())).collect();
    let place = containing(&refs, db)?;
    let mut out = Vec::new();
    for (q, misses) in missed {
        let (cands, words) = retrieve(&q.question, db, now, CANDIDATES, q.as_of, &q.prior)?;
        let real: Vec<Hit> = cands.iter().take(REAL_LIMIT).cloned().collect();
        let (groups, rest) = select(&real, &q.question);
        let selected: HashSet<String> = groups.iter().flat_map(|(_, hits)| hits.iter().map(hit_key)).collect();
        let rest: HashSet<String> = rest.iter().map(hit_key).collect();
        let truncated = utf16_len(&q.block) >= LIMIT_CHARS - 1;
        let mut global: Option<Vec<String>> = None;
        for reference in misses {
            let keys = &place[reference.as_str()];
            let base = |class| Miss { id: q.id.clone(), tier: q.tier.clone(), reference: reference.clone(), class, rank: None, cut: None, doc: None };
            if keys.is_empty() {
                out.push(base('a'));
                continue;
            }
            let mut best: Option<(&'static str, &Hit)> = None;
            for (rank, hit) in cands.iter().enumerate() {
                let key = hit_key(hit);
                if !keys.contains(&key) {
                    continue;
                }
                let cut = if rank >= REAL_LIMIT {
                    "limit"
                } else if hit.cover < FLOOR {
                    "floor"
                } else if selected.contains(&key) {
                    if truncated { "char budget" } else { "render" }
                } else if rest.contains(&key) {
                    "category cap"
                } else {
                    "dedup"
                };
                let order = |c: &str| CUT_ORDER.iter().position(|x| *x == c);
                if best.is_none_or(|(b, _)| order(cut) > order(b)) {
                    best = Some((cut, hit));
                }
            }
            if let Some((cut, hit)) = best {
                out.push(Miss { cut: Some(cut), doc: Some(format!("{}:{}", hit.source, hit.reference)), ..base('c') });
                continue;
            }
            if global.is_none() {
                global = Some(if words.is_empty() { Vec::new() } else { bm25_keys(db, q.as_of, &words)? });
            }
            let at = global.as_ref().and_then(|g| g.iter().position(|k| keys.contains(k)));
            out.push(Miss { rank: Some(at.map(|i| i + 1)), ..base('b') });
        }
    }
    Ok(out)
}

struct Counts {
    a: usize,
    b: usize,
    c: usize,
    total: usize,
    cuts: Vec<(String, usize)>,
}

fn counts(misses: &[&Miss]) -> Counts {
    let mut c = Counts { a: 0, b: 0, c: 0, total: misses.len(), cuts: Vec::new() };
    for m in misses {
        match m.class {
            'a' => c.a += 1,
            'b' => c.b += 1,
            _ => c.c += 1,
        }
        if let Some(cut) = m.cut {
            match c.cuts.iter_mut().find(|(k, _)| k == cut) {
                Some((_, n)) => *n += 1,
                None => c.cuts.push((cut.to_string(), 1)),
            }
        }
    }
    c
}

impl Counts {
    fn json(&self) -> J {
        J::obj(vec![
            ("a", J::Num(self.a as f64)),
            ("b", J::Num(self.b as f64)),
            ("c", J::Num(self.c as f64)),
            ("total", J::Num(self.total as f64)),
            ("cuts", J::Obj(self.cuts.iter().map(|(k, n)| (k.clone(), J::Num(*n as f64))).collect())),
        ])
    }

    fn line(&self) -> String {
        format!("a {} / b {} / c {} = {} misses; cuts {}", self.a, self.b, self.c, self.total,
            J::Obj(self.cuts.iter().map(|(k, n)| (k.clone(), J::Num(*n as f64))).collect()).text(None))
    }
}

impl Miss {
    fn json(&self) -> J {
        let mut pairs = vec![
            ("id", J::Str(self.id.clone())),
            ("tier", J::Str(self.tier.clone())),
            ("ref", J::Str(self.reference.clone())),
            ("class", J::Str(self.class.to_string())),
        ];
        if let Some(rank) = self.rank {
            pairs.push(("rank", rank.map_or(J::Null, |r| J::Num(r as f64))));
        }
        if let Some(cut) = self.cut {
            pairs.push(("cut", J::Str(cut.into())));
        }
        if let Some(doc) = &self.doc {
            pairs.push(("doc", J::Str(doc.clone())));
        }
        J::obj(pairs)
    }

    fn line(&self) -> String {
        let tail = match self.class {
            'b' => format!(" best-rank {}", self.rank.flatten().map_or("no query term hits it".into(), |r| r.to_string())),
            'c' => format!(" {} via {}", self.cut.unwrap_or(""), self.doc.as_deref().unwrap_or("")),
            _ => " no indexed doc contains it".into(),
        };
        format!("  {} [{}] {}: {}{tail}", self.id, self.tier, self.reference, self.class)
    }
}

fn exclude_session_tree(sessions: &[String], index: &Path) -> NorthResult<(PathBuf, Vec<(String, usize)>)> {
    let stamp = Timestamp::now().unix_nanos();
    let dir = env::temp_dir().join(format!("grounding-score-{}-{stamp}", std::process::id()));
    fs::create_dir(&dir)?;
    fs::copy(index, dir.join("index.db"))?;
    let convo = convo_db();
    let mut ids: Vec<String> = Vec::new();
    if convo.exists() {
        for s in sessions {
            let rows = sqlite::rows(&convo, Access::ReadOnly, SQLITE_BUSY, &format!(
                "select m.id from msg m join files f on f.id = m.file_id where f.session_id like {}",
                sql_text(&format!("{s}%"))
            ))?;
            ids.extend(rows.iter().map(|r| text_of(&r["id"])));
        }
    }
    let copy = dir.join("index.db");
    let mut removed = Vec::new();
    if !ids.is_empty() {
        let list = ids.iter().map(|i| sql_text(i)).collect::<Vec<_>>().join(",");
        for source in ["transcripts", "builds"] {
            let count = |copy: &Path| -> NorthResult<usize> {
                Ok(sqlite::rows(copy, Access::ReadOnly, SQLITE_BUSY, &format!("select count(*) n from docs where source = '{source}'"))?
                    .first()
                    .and_then(|r| r["n"].as_u64())
                    .unwrap_or(0) as usize)
            };
            let before = count(&copy)?;
            sqlite::execute(&copy, SQLITE_BUSY, &format!("delete from docs where source = '{source}' and id in ({list});"))?;
            removed.push((source.to_string(), before - count(&copy)?));
        }
    }
    Ok((dir, removed))
}

fn prior_for(item: &Item) -> NorthResult<String> {
    let (Some(session), Some(convo_id)) = (&item.session, item.convo_id) else { return Ok(String::new()) };
    if convo_id == 0 {
        return Ok(String::new());
    }
    let db = convo_db();
    if !db.exists() {
        return Ok(String::new());
    }
    let rows = sqlite::rows(&db, Access::ReadOnly, SQLITE_BUSY, &format!(
        "select b.text from msg m join body b on b.id=m.body join files f on f.id=m.file_id join msg current on current.id={convo_id} \
         where f.session_id like {} and m.file_id=current.file_id and m.line<current.line and m.role='assistant' and m.ts<=current.ts \
         and m.dup=0 order by m.line desc limit 1",
        sql_text(&format!("{session}%"))
    ))?;
    Ok(rows.first().map(|r| utf16_head(&text_of(&r["text"]), 4000)).unwrap_or_default())
}

struct Scored {
    rows: Vec<Row>,
    misses: Vec<Miss>,
    summary: Vec<(&'static str, J)>,
    classes_a: Counts,
    classes_all: Counts,
    holdout_file: String,
    holdout_rows: usize,
    holdout_needed: usize,
    holdout_misses: Vec<Miss>,
}

fn score(path: &Path, o: &Options) -> NorthResult<Scored> {
    let items: Vec<Item> = read_rows(path)?;
    let real = state_dir();
    let excluded = if !o.exclude.is_empty() && !o.leaky { Some(exclude_session_tree(&o.exclude, &real.join("index.db"))?) } else { None };
    let state = excluded.as_ref().map_or(real.clone(), |(dir, _)| dir.clone());
    let result = score_in(&items, path, o, excluded.as_ref().map(|(_, r)| r.clone()), &state);
    if let Some((dir, _)) = excluded {
        let _ = fs::remove_file(dir.join("index.db"));
        let _ = fs::remove_dir(&dir);
    }
    result
}

fn score_in(items: &[Item], path: &Path, o: &Options, removed: Option<Vec<(String, usize)>>, state: &Path) -> NorthResult<Scored> {
    let mut latencies = Vec::new();
    let mut rows = Vec::new();
    let mut queries = Vec::new();
    for item in items {
        let refs: Vec<String> = item.needed.iter().map(|n| n.reference.clone()).collect();
        let as_of = as_of_of(item.asked_at.as_deref(), o.leaky);
        let prior = prior_for(item)?;
        let tier = item.tier.clone().unwrap_or_else(|| "A".into());
        let mut block_a = String::new();
        for _ in 0..o.runs.max(1) {
            let (out, s) = run_query(&item.question, "A", as_of, &prior, state)?;
            block_a = out;
            latencies.push(s);
        }
        let found_a: Vec<&String> = refs.iter().filter(|r| block_a.contains(r.as_str())).collect();
        let mut found_ab = found_a.clone();
        if o.with_b && found_a.len() < refs.len() {
            let (block_b, _) = run_query(&item.question, "B", as_of, &prior, state)?;
            found_ab = refs.iter().filter(|r| block_a.contains(r.as_str()) || block_b.contains(r.as_str())).collect();
        }
        rows.push(Row {
            id: item.id.clone(),
            tier: tier.clone(),
            needed: refs.len(),
            a: found_a.len(),
            ab: found_ab.len(),
            misses_a: refs.iter().filter(|r| !found_a.contains(r)).cloned().collect(),
            misses_ab: refs.iter().filter(|r| !found_ab.contains(r)).cloned().collect(),
            chars: utf16_len(&block_a),
        });
        queries.push(Query { id: item.id.clone(), tier, question: item.question.clone(), refs, block: block_a, as_of, prior });
    }
    let hold_file = o.holdout.clone().map(PathBuf::from).unwrap_or_else(|| path.parent().unwrap_or(Path::new("")).join("misses.jsonl"));
    let hold: Vec<MissRow> = if hold_file.exists() { read_rows(&hold_file)? } else { Vec::new() };
    let mut hold_queries = Vec::new();
    for (i, row) in hold.iter().enumerate() {
        let as_of = as_of_of(row.at.as_deref(), o.leaky);
        let (block, _) = run_query(&row.query, "A", as_of, "", state)?;
        hold_queries.push(Query {
            id: format!("holdout-{i}"),
            tier: "A".into(),
            question: row.query.clone(),
            refs: row.should_have_found.clone(),
            block,
            as_of,
            prior: String::new(),
        });
    }
    let db = state.join("index.db");
    if !db.exists() {
        return Err(NorthError::State(format!("no index at {}", db.display())));
    }
    let now = Timestamp::now().unix_seconds_f64();
    let misses = diagnose(&queries, &db, now)?;
    let hold_misses = diagnose(&hold_queries, &db, now)?;
    let sum = |rs: &[&Row], k: fn(&Row) -> usize| rs.iter().map(|r| k(r)).sum::<usize>();
    let all: Vec<&Row> = rows.iter().collect();
    let tier_a: Vec<&Row> = rows.iter().filter(|r| r.tier == "A").collect();
    let ratio = |a: usize, b: usize| a as f64 / b.max(1) as f64;
    let classes_a = counts(&misses.iter().filter(|m| m.tier == "A").collect::<Vec<_>>());
    let classes_all = counts(&misses.iter().collect::<Vec<_>>());
    let summary = vec![
        ("mode", J::Str(if o.leaky { "leaky" } else { "as-of" }.into())),
        ("excludedDocs", removed.map_or(J::Null, |r| J::Obj(r.into_iter().map(|(k, n)| (k, J::Num(n as f64))).collect()))),
        ("questions", J::Num(rows.len() as f64)),
        ("tierA", J::obj(vec![
            ("questions", J::Num(tier_a.len() as f64)),
            ("found", J::Num(sum(&tier_a, |r| r.a) as f64)),
            ("needed", J::Num(sum(&tier_a, |r| r.needed) as f64)),
        ])),
        ("recallTierARows", J::Num(ratio(sum(&tier_a, |r| r.a), sum(&tier_a, |r| r.needed)))),
        ("recallAAllRows", J::Num(ratio(sum(&all, |r| r.a), sum(&all, |r| r.needed)))),
        ("recallABAllRows", if o.with_b { J::Num(ratio(sum(&all, |r| r.ab), sum(&all, |r| r.needed))) } else { J::Null }),
        ("p50", pct(&latencies, 0.5)),
        ("p95", pct(&latencies, 0.95)),
        ("samples", J::Num(latencies.len() as f64)),
    ];
    Ok(Scored {
        rows,
        misses,
        summary,
        classes_a,
        classes_all,
        holdout_file: hold_file.display().to_string(),
        holdout_rows: hold.len(),
        holdout_needed: hold_queries.iter().map(|q| q.refs.len()).sum(),
        holdout_misses: hold_misses,
    })
}

impl Row {
    fn json(&self) -> J {
        J::obj(vec![
            ("id", J::Str(self.id.clone())),
            ("tier", J::Str(self.tier.clone())),
            ("needed", J::Num(self.needed as f64)),
            ("a", J::Num(self.a as f64)),
            ("ab", J::Num(self.ab as f64)),
            ("missesA", J::strs(&self.misses_a)),
            ("missesAB", J::strs(&self.misses_ab)),
            ("chars", J::Num(self.chars as f64)),
        ])
    }
}

fn summary_without_classes(s: &Scored) -> J {
    J::Obj(s.summary.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}

fn summary_with_classes(s: &Scored) -> J {
    let mut pairs: Vec<(String, J)> = s.summary.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let at = pairs.iter().position(|(k, _)| k == "p50").unwrap_or(pairs.len());
    pairs.insert(at, ("classesAll".into(), s.classes_all.json()));
    pairs.insert(at, ("classesTierA".into(), s.classes_a.json()));
    J::Obj(pairs)
}

fn holdout_found(s: &Scored) -> usize {
    s.holdout_needed - s.holdout_misses.len()
}

fn holdout_json(s: &Scored) -> J {
    let found = holdout_found(s);
    J::obj(vec![
        ("file", J::Str(s.holdout_file.clone())),
        ("rows", J::Num(s.holdout_rows as f64)),
        ("found", J::Num(found as f64)),
        ("needed", J::Num(s.holdout_needed as f64)),
        ("recall", if s.holdout_needed > 0 { J::Num(found as f64 / s.holdout_needed as f64) } else { J::Null }),
        ("classes", counts(&s.holdout_misses.iter().collect::<Vec<_>>()).json()),
        ("misses", J::Arr(s.holdout_misses.iter().map(Miss::json).collect())),
    ])
}

fn summary_value<'a>(s: &'a Scored, key: &str) -> &'a J {
    &s.summary.iter().find(|(k, _)| *k == key).expect("summary key").1
}

fn tier_a_counts(s: &Scored) -> (String, String, String) {
    let J::Obj(t) = summary_value(s, "tierA") else { unreachable!() };
    let get = |k: &str| t.iter().find(|(n, _)| n == k).map_or(String::new(), |(_, v)| v.text(None));
    (get("found"), get("needed"), get("questions"))
}

fn fixed3(j: &J) -> String {
    match j {
        J::Num(n) => format!("{n:.3}"),
        other => other.text(None),
    }
}

pub fn main(args: &[String]) -> NorthResult<i32> {
    let valued = ["--runs", "--holdout", "--exclude-session"];
    let path = args.iter().enumerate().find(|(i, a)| !(a.starts_with("--") || *i > 0 && valued.contains(&args[i - 1].as_str()))).map(|(_, a)| a);
    let Some(path) = path else {
        eprintln!("usage: north grounding score EVAL.jsonl [--no-b] [--runs N] [--holdout MISSES.jsonl] [--leaky] [--exclude-session ID]... [--json]");
        return Ok(2);
    };
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let options = Options {
        with_b: !args.iter().any(|a| a == "--no-b"),
        runs: value("--runs").map_or(1, |r| r.parse::<f64>().map_or(0, |n| n.max(0.0) as usize)),
        holdout: value("--holdout"),
        leaky: args.iter().any(|a| a == "--leaky"),
        exclude: args.iter().enumerate().filter(|(_, a)| *a == "--exclude-session").filter_map(|(i, _)| args.get(i + 1).cloned()).collect(),
    };
    let s = score(Path::new(path), &options)?;
    if args.iter().any(|a| a == "--json") {
        let result = J::obj(vec![
            ("rows", J::Arr(s.rows.iter().map(Row::json).collect())),
            ("misses", J::Arr(s.misses.iter().map(Miss::json).collect())),
            ("summary", summary_with_classes(&s)),
            ("holdout", holdout_json(&s)),
        ]);
        println!("{}", result.text(Some(1)));
        return Ok(0);
    }
    for r in &s.rows {
        println!(
            "{} [{}]: A {}/{} A+B {}/{} chars {} misses A {} A+B {}",
            r.id, r.tier, r.a, r.needed, r.ab, r.needed, r.chars, J::strs(&r.misses_a).text(None), J::strs(&r.misses_ab).text(None)
        );
    }
    println!("misses (tier A retrieval):");
    for m in &s.misses {
        println!("{}", m.line());
    }
    let excluded = summary_value(&s, "excludedDocs").text(None);
    println!("mode {}; session-tree docs removed from the index copy: {excluded}", if options.leaky { "leaky" } else { "as-of" });
    let (found, needed, questions) = tier_a_counts(&s);
    println!("tier-A recall: {found}/{needed} = {} over {questions} tier-A questions", fixed3(summary_value(&s, "recallTierARows")));
    println!("miss classes, tier-A questions: {}", s.classes_a.line());
    println!("miss classes, all questions: {}", s.classes_all.line());
    for m in &s.holdout_misses {
        println!("{}", m.line());
    }
    let found = holdout_found(&s);
    let recall = if s.holdout_needed > 0 { format!(" = {:.3}", found as f64 / s.holdout_needed as f64) } else { String::new() };
    println!(
        "holdout recall: {found}/{}{recall} over {} rows of {} scored on their own query text; {}",
        s.holdout_needed, s.holdout_rows, s.holdout_file, counts(&s.holdout_misses.iter().collect::<Vec<_>>()).line()
    );
    println!("{}", summary_without_classes(&s).text(None));
    Ok(0)
}
