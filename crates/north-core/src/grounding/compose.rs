use super::*;
use crate::foundation::redact::redact;
use std::collections::HashSet;
use std::time::Instant;
use store::{Bound, Hit, Sqlite};
pub const LIMIT_CHARS: usize = 6000;
pub const HOOK_LIMIT_CHARS: usize = 1500;
pub const HOOK_FLOOR: f64 = 0.5;
pub const HOOK_TERMS: usize = 12;
pub struct Shape {
    pub limit: usize,
    pub floor: f64,
    pub keep: f64,
    pub mentions: usize,
    pub terms: usize,
    pub width: usize,
    pub brief: bool,
    pub seen: HashSet<String>,
    pub last_live: Option<String>,
    pub bound: Bound,
}
impl Shape {
    pub fn query() -> Self {
        Shape { limit: LIMIT_CHARS, floor: 0.2, keep: 0.2, mentions: 1600, terms: 12, width: 80, brief: false, seen: HashSet::new(), last_live: None, bound: Bound::default() }
    }
    pub fn hook(seen: HashSet<String>, last_live: Option<String>, until: Option<Instant>) -> Self {
        Shape { limit: HOOK_LIMIT_CHARS, floor: HOOK_FLOOR, keep: 0.2, mentions: 1600, terms: 6, width: 0, brief: true, seen, last_live, bound: Bound { terms: Some(HOOK_TERMS), until } }
    }
}
#[derive(Default)]
pub struct Rendered {
    pub text: String,
    pub refs: Vec<String>,
    pub live: Option<String>,
}
pub fn live_key(lines: &[String]) -> String {
    re(r", (?:in \d+[mh]|as of \d\d:\d\dZ)").replace_all(&lines.join("\n"), "").into_owned()
}
const SLOTS: [(&str, usize); 8] = [
    ("Decisions", 3),
    ("Skills", 4),
    ("Policy", 7),
    ("Handoffs", 2),
    ("Tom said", 4),
    ("Built before", 3),
    ("Projects", 3),
    ("Issues", 2),
];
pub fn category(hit: &Hit) -> String {
    match hit.source.as_str() {
        "policy" if hit.title.starts_with("handoff ") => "Handoffs",
        "policy" if hit.title.starts_with("skill ") || hit.reference.contains("/skills/") => {
            "Skills"
        }
        "policy" | "settings" => "Policy",
        "builds" | "commits" => "Built before",
        "threads" | "github" => "Issues",
        "ledger" => "Decisions",
        "transcripts" => "Tom said",
        "projects" => "Projects",
        name => name,
    }
    .into()
}
fn weight(hit: &Hit) -> f64 {
    if hit.source != "ledger" {
        return hit.score;
    }
    let mut parts = hit.title.split(' ');
    let speaker = parts.next().unwrap_or("");
    let kind = parts.next().unwrap_or("");
    hit.score
        * if kind == "direction" { 0.6 } else { 1.0 }
        * if speaker == "tom" { 1.0 } else { 0.8 }
}
pub fn select(hits: &[Hit], message: &str) -> (Vec<(String, Vec<Hit>)>, Vec<Hit>) {
    let shape = Shape::query();
    pick(hits, message, &shape, shape.keep)
}
fn pick(hits: &[Hit], message: &str, shape: &Shape, floor: f64) -> (Vec<(String, Vec<Hit>)>, Vec<Hit>) {
    let mut groups: Vec<(String, Vec<Hit>)> = Vec::new();
    let mut rest = Vec::new();
    let mut seen = HashSet::new();
    let mut handoffs: Vec<HashSet<String>> = Vec::new();
    let mut ordered: Vec<_> = hits.iter().collect();
    ordered.sort_by(|a, b| (shape.brief && code(a)).cmp(&(shape.brief && code(b))).then(weight(b).total_cmp(&weight(a))));
    let norm = cut(&flat(message), 160);
    for hit in ordered {
        let same = (hit.source == "transcripts" || hit.source == "ledger")
            && !norm.is_empty()
            && cut(&flat(&hit.text), 160) == norm;
        if hit.cover < floor || seen.contains(&hit.reference) || shape.seen.contains(&hit.reference) || same {
            continue;
        }
        let name = category(hit);
        if name == "Handoffs" {
            let words: HashSet<_> = re(r"[a-z][a-z0-9_-]*")
                .find_iter(&flat(&hit.text).to_lowercase())
                .map(|m| m.as_str().to_owned())
                .collect();
            if handoffs.iter().any(|prior| {
                let n = prior.intersection(&words).count() as f64;
                !words.is_empty()
                    && !prior.is_empty()
                    && n / words.len().min(prior.len()) as f64 >= 0.9
                    && n / words.len().max(prior.len()) as f64 >= 0.8
            }) {
                continue;
            }
            handoffs.push(words);
        }
        seen.insert(hit.reference.clone());
        if !groups.iter().any(|(n, _)| n == &name) {
            groups.push((name.clone(), vec![]));
        }
        let bucket = &mut groups.iter_mut().find(|(n, _)| n == &name).unwrap().1;
        let max = SLOTS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, n)| *n)
            .unwrap_or(2);
        if bucket.len() < max {
            bucket.push(hit.clone());
        } else {
            rest.push(hit.clone());
        }
    }
    (groups, rest)
}
fn snippet(text: &str, words: &[String], width: usize) -> String {
    let f = flat(text);
    let low = f.to_lowercase();
    let pos = words.iter().filter_map(|w| low.find(w)).min();
    let start = pos.map(|p| ulen(&low[..p]).saturating_sub(30)).unwrap_or(0);
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        slice(&f, start, width),
        if start + width < ulen(&f) { "…" } else { "" }
    )
}
fn outcome(hit: &Hit) -> String {
    if hit.source == "commits" {
        return "landed".into();
    }
    let mut shas = Vec::new();
    for m in re(r"\b[0-9a-f]{8,40}\b").find_iter(&hit.text).take(3) {
        let h = cut(m.as_str(), 8);
        if !shas.contains(&h) {
            shas.push(h);
        }
    }
    let head = cut(&hit.text, 200).to_lowercase();
    let state = if re(r"^\W*(not done|blocked)").is_match(&head) {
        "not finished"
    } else if re(r"(?i)\b(landed|merged|pushed|queued)\b").is_match(&hit.text) {
        "landed"
    } else {
        "outcome unknown"
    };
    if shas.is_empty() {
        state.into()
    } else {
        format!("{state} {}", shas.join(" "))
    }
}
fn clipped(text: &str, width: usize) -> String {
    if ulen(text) > width { format!("{}…", cut(text, width)) } else { text.to_owned() }
}
fn located(hit: &Hit) -> Option<Option<&str>> {
    let (label, heading) = hit.title.split_once(" § ").map_or((hit.title.as_str(), None), |(l, h)| (l, Some(h)));
    let path = label.split_once(':').map(|(_, p)| p)?;
    (path.contains('/') && hit.reference.ends_with(&format!("/{path}"))).then_some(heading)
}
pub fn shown(reference: &str) -> String {
    let Some(rest) = reference.strip_prefix("~/code/") else { return reference.to_owned() };
    match rest.split_once('/').and_then(|(repo, r)| Some((repo, r.split_once('/')?.1))) {
        Some((repo, path)) => format!("{repo}:{path}"),
        None => reference.to_owned(),
    }
}
fn same_place(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}
fn hit_line(hit: &Hit, shape: &Shape) -> String {
    let width = shape.width;
    if hit.source == "transcripts" {
        return format!(
            "- {} ({}): {}",
            hit.reference,
            hit.title,
            snippet(&hit.text, &hit.matched, width.max(48))
        );
    }
    if hit.source == "builds" || hit.source == "commits" {
        return format!(
            "- {} {} [{}]",
            hit.reference,
            cut(&hit.title, 80),
            outcome(hit)
        );
    }
    let why = if width > 0 && !hit.text.is_empty() && !hit.title.starts_with("skill ") {
        snippet(&hit.text, &hit.matched, width)
    } else {
        String::new()
    };
    let head = match located(hit) {
        Some(Some(heading)) => format!("{} § {}", shown(&hit.reference), clipped(heading, if shape.brief { 32 } else { 60 })),
        Some(None) => shown(&hit.reference),
        None => {
            let title = clipped(&hit.title, if shape.brief && !hit.title.starts_with("skill ") { 32 } else { 80 });
            if title.contains(&hit.reference) { title } else { format!("{} — {title}", shown(&hit.reference)) }
        }
    };
    format!(
        "- {head}{}",
        if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        }
    )
}
fn mentions(hits: &[Hit]) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = vec![];
    let mut placements = Vec::new();
    for hit in hits {
        for cap in re(r"\b(hook|skill):([a-z0-9_-]+)").captures_iter(&hit.title) {
            let p = if &cap[1] == "hook" {
                format!("harness/hooks/{}.sh", &cap[2])
            } else {
                format!("harness/skills/{}/SKILL.md", &cap[2])
            };
            if !placements.contains(&p) {
                placements.push(p);
            }
        }
        let text = format!("{}\n{}\n{}", hit.reference, hit.title, hit.text);
        let mut refs = Vec::new();
        for m in re(r"(?:~/|/)?(?:[\w.@-]+/)+[\w.@-]+(?:#[\w:-]+)?|\b(?:convo:\d+|rule:[\w-]+|[a-z][\w-]*@[0-9a-f]{8,40}|[0-9a-f]{8,40})\b").find_iter(&text) {
            let r = m.as_str().trim_end_matches(['.', ',']);
            if r.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if r.contains('/') && !r.contains(['.', '#']) && !re(r"^(?:[~/]|harness/|agents/|lib/|bin/|docs/|skills/)").is_match(r) {
                continue;
            }
            if !refs.contains(&r) {
                refs.push(r);
            }
        }
        for r in refs {
            if let Some((_, n)) = counts.iter_mut().find(|(v, _)| v == r) {
                *n += 1;
            } else {
                counts.push((r.to_owned(), 1));
            }
        }
    }
    counts.sort_by_key(|count| std::cmp::Reverse(count.1));
    for (r, _) in counts {
        if placements.len() >= 30 {
            break;
        }
        push_distinct(&mut placements, r);
    }
    placements
}
fn push_distinct(places: &mut Vec<String>, place: String) {
    if !places.iter().any(|p| same_place(p, &place)) {
        places.push(place);
    }
}
pub fn compose(
    message: &str,
    hits: &[Hit],
    words: &[String],
    live: &[String],
    extra: &[String],
    shape: &Shape,
    floor: f64,
) -> (String, Vec<String>) {
    let mut lines = opening(words, live, extra, shape);
    let (groups, rest) = pick(hits, message, shape, floor);
    let selected: Vec<_> = groups.iter().flat_map(|(_, h)| h.clone()).collect();
    let named: Vec<_> = mentions(&selected)
        .into_iter()
        .filter(|r| !shape.seen.contains(r) && !selected.iter().any(|h| same_place(&h.reference, r)))
        .collect();
    let entities = named.iter().map(|r| shown(r)).collect::<Vec<_>>().join(", ");
    let mentioned = (!entities.is_empty()).then(|| format!("Mentions: {}", cut(&entities, shape.mentions)));
    if !shape.brief {
        lines.extend(mentioned.clone());
    }
    for (name, _) in SLOTS {
        if let Some((_, items)) = groups.iter().find(|(n, _)| n == name)
            && !items.is_empty() {
                lines.push(format!("{name}:"));
                lines.extend(items.iter().map(|h| hit_line(h, shape)));
            }
    }
    if !shape.brief {
        lines.extend_from_slice(extra);
    }
    let more: Vec<_> = rest
        .iter()
        .filter(|h| h.source != "builds" && h.source != "commits")
        .take(20)
        .map(|h| h.reference.clone())
        .collect();
    if !more.is_empty() {
        lines.push(format!("Also matching: {}", more.iter().map(|r| shown(r)).collect::<Vec<_>>().join(", ")));
    }
    if shape.brief {
        lines.extend(mentioned);
    }
    let text = fit(lines, shape.limit);
    let mut refs: Vec<String> = selected.iter().map(|h| h.reference.clone()).chain(named).chain(more).collect();
    refs.retain(|r| !r.is_empty() && text.contains(shown(r).as_str()));
    refs.dedup();
    (text, refs)
}
pub fn retrieve(
    message: &str,
    db: &Path,
    now: f64,
    limit: usize,
    as_of: f64,
    prior: &str,
) -> NorthResult<(Vec<Hit>, Vec<String>)> {
    let run = || -> Result<_> {
        let mut db = Sqlite::open(db, true)?;
        retrieve_db(message, &mut db, now, limit, as_of, prior)
    };
    run().map_err(|e| NorthError::Configuration(e.to_string()))
}
pub(super) fn retrieve_db(
    message: &str,
    db: &mut Sqlite,
    now: f64,
    limit: usize,
    as_of: f64,
    prior: &str,
) -> Result<(Vec<Hit>, Vec<String>)> {
    let (mut hits, words) = store::search(message, db, now, limit, as_of)?;
    if !prior.is_empty() && !db.expired() {
        let current: HashSet<_> = store::terms(message).into_iter().collect();
        let context = store::terms(prior)
            .into_iter()
            .filter(|w| !current.contains(w))
            .take(16)
            .collect::<Vec<_>>()
            .join(" ");
        if !context.is_empty() {
            let known: HashSet<_> = hits
                .iter()
                .map(|h| format!("{}:{}", h.source, h.id))
                .collect();
            hits.extend(
                store::search(&context, db, now, limit, as_of)?
                    .0
                    .into_iter()
                    .filter(|h| !known.contains(&format!("{}:{}", h.source, h.id)))
                    .map(|mut h| {
                        h.score *= 0.4;
                        h
                    }),
            );
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok((hits, words))
}
pub fn block(message: &str, tier: &str, now: f64, prior: &str) -> Result<String> {
    Ok(shaped(message, tier, now, prior, &Shape::query())?.text)
}
pub fn shaped(message: &str, tier: &str, now: f64, prior: &str, shape: &Shape) -> Result<Rendered> {
    let usage = live::usage(message);
    let org = live::org_query(message);
    let count = store::terms(message).len();
    if count < 2 && (prior.is_empty() || count == 0) && !usage && !org {
        return Ok(Rendered::default());
    }
    let mut db = store::open(true).ok();
    let mut hits = Vec::new();
    let mut words = store::terms(message);
    let as_of = env::var("NORTH_GROUNDING_AS_OF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    if let Some(db) = db.as_mut() {
        db.bound(shape.bound);
    }
    if let Some(db) = db.as_mut()
        && let Ok((h, w)) = retrieve_db(message, db, now, 80, as_of, prior) {
            hits = h;
            words = w;
        }
    if words.is_empty() {
        words = store::terms(message);
    }
    let mut live = if usage { live::pace_lines() } else { vec![] };
    if org {
        live.extend(live::org_lines(message));
    }
    let confident = hits.iter().any(|h| h.cover >= 0.5);
    let mut extra = Vec::new();
    if tier=="B"{let(lines,searched)=tierb::tier_b(&words,db.as_mut());if !lines.is_empty(){extra.push("Tier B (convo rephrasings, handoffs):".into());extra.extend(lines.into_iter().take(14));}else if !confident{let near=hits.first().map(|h|format!("closest: {}{}",h.reference,if h.at!=0.0{format!(" on {}",Timestamp::from_unix_seconds(h.at as i64).day_and_minute())}else{String::new()})).unwrap_or_else(||"closest: none".into());extra.push(format!("Ask Tom in this form: \"searched {} for '{}'; {near}; this one?\"",searched.join(", "),words.iter().take(6).cloned().collect::<Vec<_>>().join(" ")));}}
    else if !confident&&re(r"(?i)\b(already|again|before|earlier|last time|previously|recently|yesterday|remind|told you|we (did|have|had|talked|discussed|agreed|decided|built|made|used)|did(n'?t| not) (we|i|you)|don'?t we|do we (not )?have|have we|existing|remember|as i said|i said|i asked|(the|our|a|any) (skill|hook|rule|routine|policy|script|tool|protocol|system|index|ledger|guard)s?|why (was|is|did|didn'?t|don'?t)|how (does|do|did) (we|the|our|it))\b").is_match(message){extra.push(format!("No confident tier-A hit and this refers to past work or existing infrastructure: before answering run tier B (<=60 s): `north grounding query --tier B --query '{}'`",cut(&message.replace('\''," "),(shape.limit / 20).min(300))));}
    Ok(assemble(message, &hits, &words, live, &extra, shape))
}
pub fn assemble(message: &str, hits: &[Hit], words: &[String], mut live: Vec<String>, extra: &[String], shape: &Shape) -> Rendered {
    let key = (!live.is_empty()).then(|| live_key(&live));
    if key.is_some() && key == shape.last_live {
        live.clear();
    }
    let fresh = hits.iter().any(|h| h.cover >= shape.floor && !shape.seen.contains(&h.reference));
    if !fresh && live.is_empty() && extra.is_empty() {
        return Rendered { live: key, ..Rendered::default() };
    }
    let floor = if fresh || !extra.is_empty() { shape.keep } else { shape.floor };
    let (text, refs) = compose(message, hits, words, &live, extra, shape, floor);
    Rendered { text: redact(&text), refs: refs.iter().map(|r| redact(r)).collect(), live: key }
}
pub fn indexed(message: &str, db: &mut Sqlite, now: f64, shape: &Shape) -> Result<Rendered> {
    let (hits, words) = retrieve_db(message, db, now, 80, 0.0, "")?;
    Ok(assemble(message, &hits, &words, Vec::new(), &[], shape))
}
pub(super) fn code(hit: &Hit) -> bool {
    hit.source == "policy" && re(r"\.(rs|ts|mjs|js|sh|py|nix)$|/bin/[^/]+$").is_match(&hit.reference)
}
fn opening(words: &[String], live: &[String], extra: &[String], shape: &Shape) -> Vec<String> {
    let terms = words.iter().take(shape.terms).cloned().collect::<Vec<_>>().join(" ");
    if !shape.brief {
        let mut lines = vec![format!("Grounding (auto-retrieved for this message; terms: {terms}). Read the named files before answering; say what already exists before proposing anything new.")];
        lines.extend_from_slice(live);
        return lines;
    }
    let mut lines = vec![format!("Grounding (terms: {terms}). Read the named files before answering; say what exists before proposing anything new.")];
    lines.extend(live.iter().map(|l| re(r"\(resets [^,()]+, (in \d+[mh]), as of [^)]+\)").replace_all(l, "($1)").into_owned()));
    lines.extend_from_slice(extra);
    lines
}
fn fit(lines: Vec<String>, limit: usize) -> String {
    let mut out = Vec::new();
    let mut size = 0;
    for mut line in lines {
        if size + ulen(&line) + 1 > limit {
            line = cut(&line, limit.saturating_sub(size + 1));
        }
        if line.is_empty() {
            break;
        }
        size += ulen(&line) + 1;
        out.push(line);
    }
    out.join("\n")
}
