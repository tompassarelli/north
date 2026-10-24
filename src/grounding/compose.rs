use super::*;
use std::collections::HashSet;
use store::{Hit, Sqlite};
pub const LIMIT_CHARS: usize = 6000;
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
    let mut groups: Vec<(String, Vec<Hit>)> = Vec::new();
    let mut rest = Vec::new();
    let mut seen = HashSet::new();
    let mut handoffs: Vec<HashSet<String>> = Vec::new();
    let mut ordered: Vec<_> = hits.iter().collect();
    ordered.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
    for hit in ordered {
        let norm = cut(&flat(message), 160);
        let same = (hit.source == "transcripts" || hit.source == "ledger")
            && !norm.is_empty()
            && cut(&flat(&hit.text), 160) == norm;
        if hit.cover < 0.2 || seen.contains(&hit.reference) || same {
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
fn hit_line(hit: &Hit) -> String {
    if hit.source == "transcripts" {
        return format!(
            "- {} ({}): {}",
            hit.reference,
            hit.title,
            snippet(&hit.text, &hit.matched, 80)
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
    let why = if !hit.text.is_empty() && !hit.title.starts_with("skill ") {
        snippet(&hit.text, &hit.matched, 80)
    } else {
        String::new()
    };
    let title = if ulen(&hit.title) > 80 {
        format!("{}…", cut(&hit.title, 80))
    } else {
        hit.title.clone()
    };
    let head = if title.contains(&hit.reference) {
        title
    } else {
        format!("{} — {title}", hit.reference)
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
        for m in re(r"(?:~/|/)?(?:[\w.@-]+/)+[\w.@-]+(?:#[\w:-]+)?|\b(?:convo:\d+|rule:[\w-]+|[a-z][\w-]*@[0-9a-f]{8,40}|[0-9a-f]{8,40})\b").find_iter(&text){let r=m.as_str().trim_end_matches(['.',',']);if r.chars().all(|c|c.is_ascii_digit()){continue;}if r.contains('/')&&!r.contains(['.','#'])&&!re(r"^(?:[~/]|harness/|agents/|lib/|bin/|docs/|skills/)").is_match(r){continue;}if !refs.contains(&r){refs.push(r);}}
        for r in refs {
            if let Some((_, n)) = counts.iter_mut().find(|(v, _)| v == r) {
                *n += 1;
            } else {
                counts.push((r.to_owned(), 1));
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    let n = 30usize.saturating_sub(placements.len());
    let extra: Vec<_> = counts
        .into_iter()
        .filter(|(r, _)| !placements.contains(r))
        .take(n)
        .map(|(r, _)| r)
        .collect();
    placements.extend(extra);
    placements
}
pub fn compose(
    message: &str,
    hits: &[Hit],
    words: &[String],
    live: &[String],
    extra: &[String],
) -> String {
    let mut lines = vec![format!(
        "Grounding (auto-retrieved for this message; terms: {}). Read the named files before answering; say what already exists before proposing anything new.",
        words.iter().take(12).cloned().collect::<Vec<_>>().join(" ")
    )];
    lines.extend_from_slice(live);
    let (groups, rest) = select(hits, message);
    let selected: Vec<_> = groups.iter().flat_map(|(_, h)| h.clone()).collect();
    let entities = mentions(&selected).join(", ");
    if !entities.is_empty() {
        lines.push(format!("Mentions: {}", cut(&entities, 1600)));
    }
    for (name, _) in SLOTS {
        if let Some((_, items)) = groups.iter().find(|(n, _)| n == name)
            && !items.is_empty() {
                lines.push(format!("{name}:"));
                lines.extend(items.iter().map(hit_line));
            }
    }
    lines.extend_from_slice(extra);
    let more: Vec<_> = rest
        .iter()
        .filter(|h| h.source != "builds" && h.source != "commits")
        .take(20)
        .map(|h| h.reference.clone())
        .collect();
    if !more.is_empty() {
        lines.push(format!("Also matching: {}", more.join(", ")));
    }
    let mut out = Vec::new();
    let mut size = 0;
    for mut line in lines {
        if size + ulen(&line) + 1 > LIMIT_CHARS {
            line = cut(&line, LIMIT_CHARS.saturating_sub(size + 1));
        }
        if line.is_empty() {
            break;
        }
        size += ulen(&line) + 1;
        out.push(line);
    }
    out.join("\n")
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
fn retrieve_db(
    message: &str,
    db: &mut Sqlite,
    now: f64,
    limit: usize,
    as_of: f64,
    prior: &str,
) -> Result<(Vec<Hit>, Vec<String>)> {
    let (mut hits, words) = store::search(message, db, now, limit, as_of)?;
    if !prior.is_empty() {
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
    let usage = live::usage(message);
    let org = live::org_query(message);
    let count = store::terms(message).len();
    if count < 2 && (prior.is_empty() || count == 0) && !usage && !org {
        return Ok(String::new());
    }
    let mut db = store::open(true).ok();
    let mut hits = Vec::new();
    let mut words = store::terms(message);
    let as_of = env::var("NORTH_GROUNDING_AS_OF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
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
    else if !confident&&re(r"(?i)\b(already|again|before|earlier|last time|previously|recently|yesterday|remind|told you|we (did|have|had|talked|discussed|agreed|decided|built|made|used)|did(n'?t| not) (we|i|you)|don'?t we|do we (not )?have|have we|existing|remember|as i said|i said|i asked|(the|our|a|any) (skill|hook|rule|routine|policy|script|tool|protocol|system|index|ledger|guard)s?|why (was|is|did|didn'?t|don'?t)|how (does|do|did) (we|the|our|it))\b").is_match(message){extra.push(format!("No confident tier-A hit and this refers to past work or existing infrastructure: before answering run tier B (<=60 s): `north grounding query --tier B --query '{}'`",cut(&message.replace('\''," "),300)));}
    if !hits.iter().any(|h| h.cover >= 0.2) && live.is_empty() && extra.is_empty() {
        return Ok(String::new());
    }
    Ok(redact(&compose(message, &hits, &words, &live, &extra)))
}
