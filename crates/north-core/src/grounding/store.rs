use super::*;
use north_graph::{Field, Graph, TextHit, TextSearch, Term, Triple};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub const WRITER: &str = "grounding";
const SOURCE: &str = "doc-source";
const REF: &str = "doc-ref";
const TITLE: &str = "doc-title";
const TEXT: &str = "doc-text";
const AT: &str = "doc-at";
const DECAYS: &str = "doc-decays";
const FAMILY: &str = "doc-family";
const HISTORY: [&str; 6] = ["transcripts", "builds", "ledger", "commits", "threads", "github"];
pub const SOURCES: [&str; 9] = ["builds", "commits", "github", "ledger", "policy", "projects", "settings", "threads", "transcripts"];
const PATIENCE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub source: String,
    pub id: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: String,
    pub text: String,
    pub at: f64,
    pub decays: bool,
}
impl Doc {
    pub fn subject(&self) -> Term {
        subject(&self.source, &self.id)
    }
    pub fn family(&self) -> &'static str {
        family(&self.source, &self.title, &self.reference)
    }
    pub fn facts(&self) -> Vec<Triple> {
        let subject = self.subject();
        let fact = |predicate: &str, object: Term| Triple::new(subject.clone(), Term::keyword(predicate), object);
        let mut facts = vec![
            fact(SOURCE, Term::string(&self.source)),
            fact(TITLE, Term::string(&self.title)),
            fact(TEXT, Term::string(&self.text)),
            fact(AT, Term::float(self.at)),
            fact(FAMILY, Term::string(self.family())),
        ];
        if default_ref(&self.source, &self.id).as_deref() != Some(self.reference.as_str()) {
            facts.push(fact(REF, Term::string(&self.reference)));
        }
        if self.decays {
            facts.push(fact(DECAYS, Term::boolean(true)));
        }
        facts
    }
    pub fn from_facts<'a>(subject: &Term, facts: impl IntoIterator<Item = &'a Triple>) -> Option<Doc> {
        let mut doc = Doc { source: String::new(), id: String::new(), reference: String::new(), title: String::new(), text: String::new(), at: 0.0, decays: false };
        for fact in facts {
            let Term::Atom(north_graph::Atom::Keyword(predicate)) = &fact.t2 else { continue };
            match (predicate.as_str(), &fact.t3) {
                (SOURCE, object) => doc.source = string(object)?,
                (REF, object) => doc.reference = string(object)?,
                (TITLE, object) => doc.title = string(object)?,
                (TEXT, object) => doc.text = string(object)?,
                (AT, Term::Atom(north_graph::Atom::Float(at))) => doc.at = at.value(),
                (DECAYS, Term::Atom(north_graph::Atom::Bool(decays))) => doc.decays = *decays,
                _ => {}
            }
        }
        doc.id = string(subject)?.strip_prefix(&format!("doc:{}:", doc.source))?.to_owned();
        if doc.reference.is_empty() {
            doc.reference = default_ref(&doc.source, &doc.id).unwrap_or_default();
        }
        Some(doc)
    }
}
fn default_ref(source: &str, id: &str) -> Option<String> {
    match source {
        "transcripts" | "builds" => Some(format!("convo:{id}")),
        "ledger" => Some(format!("ledger:{id}")),
        "github" => Some(format!("github {id}")),
        "threads" => Some(format!("threads {id}")),
        _ => None,
    }
}
pub fn subject(source: &str, id: &str) -> Term {
    Term::string(format!("doc:{source}:{id}"))
}
pub fn source_predicate() -> Term {
    Term::keyword(SOURCE)
}
fn string(term: &Term) -> Option<String> {
    match term {
        Term::Atom(north_graph::Atom::String(text)) => Some(text.clone()),
        _ => None,
    }
}
pub fn family(source: &str, title: &str, reference: &str) -> &'static str {
    let title = title.to_ascii_lowercase();
    match source {
        "policy" if title.starts_with("handoff ") => "handoffs",
        "settings" => "config",
        "policy" if reference.to_ascii_lowercase().ends_with(".toml") && !title.starts_with("rule ") && !title.starts_with("hook ") => "config",
        "policy" => "catalog",
        "commits" | "builds" => "commits/builds",
        "threads" | "github" => "issues",
        "transcripts" => "transcripts",
        "ledger" => "ledger",
        "projects" => "projects",
        _ => "other",
    }
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub doc: Doc,
    pub cover: f64,
    pub score: f64,
    pub matched: Vec<String>,
}
impl std::ops::Deref for Hit {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.doc
    }
}
#[derive(Clone, Copy, Default)]
pub struct Bound {
    pub terms: Option<usize>,
    pub until: Option<Instant>,
}
impl Bound {
    pub fn expired(&self) -> bool {
        self.until.is_some_and(|at| Instant::now() >= at)
    }
}
pub struct Index {
    graph: Graph,
    bound: Bound,
    excluded: HashSet<String>,
}
impl Index {
    pub fn open() -> Result<Self> {
        Self::open_at(&north_graph::default_path())
    }
    pub fn open_at(path: &Path) -> Result<Self> {
        let excluded = env::var_os("NORTH_GROUNDING_EXCLUDE").filter(|file| !file.is_empty()).map(|file| read(Path::new(&file)).lines().map(str::to_owned).collect()).unwrap_or_default();
        Ok(Self { graph: Graph::open_read_only(path, PATIENCE)?, bound: Bound::default(), excluded })
    }
    pub fn excluding(mut self, file: &Path) -> Self {
        self.excluded = read(file).lines().map(str::to_owned).collect();
        self
    }
    pub fn docs_of(&self, source: &str) -> Result<Vec<Doc>> {
        let mut facts: HashMap<Term, Vec<Triple>> = HashMap::new();
        for told in self.graph.described(&source_predicate(), &Term::string(source))? {
            facts.entry(told.triple.t1.clone()).or_default().push(told.triple);
        }
        Ok(facts.iter().filter_map(|(subject, facts)| Doc::from_facts(subject, facts)).collect())
    }
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    pub fn bound(&mut self, bound: Bound) {
        self.bound = bound;
    }
    pub fn expired(&self) -> bool {
        self.bound.expired()
    }
    pub fn docs(&self) -> Result<u64> {
        Ok(self.graph.text_rows()?)
    }
    pub fn doc(&self, subject: &Term) -> Result<Option<Doc>> {
        let live = self.graph.show(subject)?;
        Ok(Doc::from_facts(subject, live.iter().map(|fact| &fact.triple)))
    }
    pub fn matching(&self, query: &str) -> Result<u64> {
        Ok(self.graph.text_matches(query)?)
    }
    pub(super) fn ranked(&self, query: &str, field: Option<Field>, as_of: f64, cap: usize) -> Result<Vec<(String, String, Option<String>)>> {
        let open = as_of == 0.0 && self.excluded.is_empty();
        let whole = open && field.is_none();
        let facets: &[&str] = if whole { &[FAMILY] } else { &[FAMILY, SOURCE, REF, AT, DECAYS] };
        let hits = self.graph.text_search(&TextSearch { query, field, weights: [4.0, 1.0], facets, cap: whole.then_some(cap) })?;
        let mut counts = HashMap::<String, usize>::new();
        let mut out = Vec::new();
        for TextHit { subject, facets, .. } in hits {
            let Some(id) = string(&subject) else { continue };
            if !whole && self.hidden(&id, &facets, as_of) {
                continue;
            }
            let family = facets[0].as_ref().and_then(string).unwrap_or_default();
            let reference = facets.get(2).and_then(Option::as_ref).and_then(string);
            if field.is_some() {
                if matches!(facets[1].as_ref().and_then(string).as_deref(), Some("policy" | "projects")) {
                    out.push((id, family, reference));
                }
                if out.len() >= cap {
                    break;
                }
                continue;
            }
            let rank = counts.entry(family.clone()).or_default();
            if *rank < cap {
                *rank += 1;
                out.push((id, family, reference));
            }
        }
        Ok(out)
    }
    fn hidden(&self, id: &str, facets: &[Option<Term>], as_of: f64) -> bool {
        if self.excluded.contains(id) {
            return true;
        }
        let source = facets[1].as_ref().and_then(string).unwrap_or_default();
        let decays = matches!(facets[4], Some(Term::Atom(north_graph::Atom::Bool(true))));
        let at = match &facets[3] {
            Some(Term::Atom(north_graph::Atom::Float(at))) => at.value(),
            _ => 0.0,
        };
        as_of != 0.0 && (decays || HISTORY.contains(&source.as_str())) && at > as_of
    }
}
const STOP: &str = "a about above after again all also am an and any are aren't as at be because been before being below between both but by can can't could couldn't did didn't do does doesn't doing don don't down during each few for from further had hadn't has hasn't have haven't having he her here hers herself him himself his how i i'm if in into is isn't it it's its itself just let's like me more most my myself no nor not now of off on once only or other ought our ours ourselves out over own same she should shouldn shouldn't so some such than that that's the their theirs them themselves then there there's these they this those through to too under until up very was wasn't we we're were weren't what what's when where which while who whom why why's with won't would wouldn't you you're your yours yourself yourselves ok okay yes yeah yep no nope thanks thank please sure right well um uh oh hey hi mean know think want need get got going go gonna kind sort thing things really actually basically just like lot also still even ever way make made use used using one two see say said tell told ask asked didn im dont didnt doesnt isnt youre thats whats ive weve theyre cant wont shouldnt wouldnt couldnt point question something anything everything someone stuff guess maybe probably pretty bit already again thought tonight today yesterday earlier recently idea fucking honestly literally";
pub fn raw_terms(text: &str) -> Vec<(String, bool)> {
    let stop: HashSet<&str> = STOP.split_whitespace().collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in re(r"[a-z0-9][a-z0-9_.\-/#:]*[a-z0-9]|[a-z0-9]").find_iter(&text.to_lowercase()) {
        let raw = raw.as_str();
        let distinct =
            re(r"[0-9]|[:_/#.\-]").is_match(raw) && !raw.chars().all(|c| c.is_ascii_digit());
        for word in raw.split(|c: char| !c.is_ascii_alphanumeric()) {
            if word.is_empty()
                || stop.contains(word)
                || seen.contains(word)
                || (word.len() < 2 && !word.chars().all(|c| c.is_ascii_digit()))
            {
                continue;
            }
            seen.insert(word.to_owned());
            out.push((word.to_owned(), distinct));
        }
    }
    out
}
pub fn terms(text: &str) -> Vec<String> {
    raw_terms(text).into_iter().map(|(w, _)| w).collect()
}
pub fn match_term(s: &str) -> String {
    format!("\"{}\"", s.replace('"', ""))
}
pub fn clauses(text: &str) -> Vec<String> {
    let joined = re(r"(?i)\s+(?:but|and then|so that|because|although|which|however)\s+")
        .replace_all(text, "\u{1f}");
    let mut parts = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = joined.char_indices().collect();
    for (n, &(i, c)) in chars.iter().enumerate() {
        if c == '\u{1f}' || (c.is_whitespace() && n > 0 && ".?!;:\n".contains(chars[n - 1].1)) {
            parts.push(joined[start..i].to_owned());
            start = i + c.len_utf8();
        }
    }
    parts.push(joined[start..].to_owned());
    let mut merged: Vec<String> = Vec::new();
    for part in parts {
        if part.split_whitespace().count() < 2 {
            continue;
        }
        if merged
            .last()
            .is_some_and(|s| s.split_whitespace().count() < 6)
        {
            merged.last_mut().unwrap().push_str(&format!(" {part}"));
        } else {
            merged.push(part);
        }
    }
    merged.truncate(10);
    merged
}
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<_> = a.encode_utf16().collect();
    let b: Vec<_> = b.encode_utf16().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut next = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            next.push(
                (next[j] + 1)
                    .min(row[j + 1] + 1)
                    .min(row[j] + usize::from(x != y)),
            );
        }
        row = next;
    }
    row[b.len()]
}
const FAMILIES: [&str; 8] = [
    "catalog",
    "config",
    "handoffs",
    "projects",
    "transcripts",
    "ledger",
    "commits/builds",
    "issues",
];
const QUERIES: usize = 3;
const REPAIRS: usize = 2;
const SHARP_DOCS: f64 = 500.0;
const MIN_TERMS: usize = 4;
const CAP: usize = 80;

fn rarest(candidates: Vec<(String, bool)>, stems: &HashMap<String, String>, df: &HashMap<String, f64>, total: f64, keep: usize) -> Vec<(String, bool)> {
    let count = |word: &str| stems.get(word).and_then(|stem| df.get(stem)).copied();
    let mut known: Vec<_> = candidates.iter().filter_map(|(word, _)| count(word).filter(|n| *n > 0.0 && !(total > 1000.0 && *n > total * 0.2)).map(|n| (word.clone(), n))).collect();
    known.sort_by(|a, b| a.1.total_cmp(&b.1));
    let sharp = known.iter().take_while(|(_, n)| *n <= SHARP_DOCS).count();
    let mut kept: HashSet<String> = known.into_iter().take(sharp.max(MIN_TERMS).min(keep)).map(|(word, _)| word).collect();
    kept.extend(candidates.iter().filter(|(word, _)| word.len() >= 6 && count(word).is_none()).take(REPAIRS).map(|(word, _)| word.clone()));
    candidates.into_iter().filter(|(word, _)| kept.contains(word)).collect()
}
fn frequencies(index: &Index, stems: &HashMap<String, String>) -> Result<HashMap<String, f64>> {
    let wanted: Vec<&str> = stems.values().map(String::as_str).collect();
    Ok(index.graph.text_frequencies(&wanted)?.into_iter().map(|(stem, n)| (stem, n as f64)).collect())
}
fn domain(index: &Index, stems: &HashMap<String, String>) -> Result<HashMap<String, (f64, f64)>> {
    let wanted: Vec<&str> = stems.values().map(String::as_str).collect();
    let mut out = HashMap::<String, (f64, f64)>::new();
    for (stem, family, n) in index.graph.text_frequencies_by(&wanted, FAMILY)? {
        let entry = out.entry(stem).or_default();
        match string(&family).as_deref() {
            Some("catalog" | "config") => entry.0 += n as f64,
            Some("projects") => entry.1 += n as f64,
            _ => {}
        }
    }
    Ok(out)
}
pub fn search(query: &str, index: &mut Index, mut now: f64, limit: usize, as_of: f64) -> Result<(Vec<Hit>, Vec<String>)> {
    if as_of != 0.0 {
        now = now.min(as_of);
    }
    let candidates: Vec<_> = raw_terms(query).into_iter().take(60).collect();
    if candidates.is_empty() {
        return Ok((vec![], vec![]));
    }
    let total = (index.docs()? as f64).max(1.0);
    let words: Vec<&str> = candidates.iter().map(|(w, _)| w.as_str()).collect();
    let mut stems = HashMap::<String, String>::new();
    for ((word, _), stem) in candidates.iter().zip(index.graph.text_stems(&words)?) {
        if let Some(stem) = stem {
            stems.insert(word.clone(), stem);
        }
    }
    let mut df = frequencies(index, &stems)?;
    let candidates = match index.bound.terms {
        Some(keep) => rarest(candidates, &stems, &df, total, keep),
        None => candidates,
    };
    stems.retain(|word, _| candidates.iter().any(|(kept, _)| kept == word));
    let mut repaired = HashMap::new();
    for (word, _) in &candidates {
        let Some(stem) = stems.get(word) else { continue };
        if df.contains_key(stem) || word.len() < 6 || index.bound.expired() {
            continue;
        }
        let mut best = 3;
        for (term, _) in index.graph.text_spelling_candidates(stem, 2, 2000)? {
            let edits = distance(stem, &term);
            if edits < best {
                best = edits;
                repaired.insert(word.clone(), term);
            }
        }
    }
    for (word, term) in &repaired {
        stems.insert(word.clone(), term.clone());
    }
    if !repaired.is_empty() {
        df = frequencies(index, &stems)?;
    }
    let domain = domain(index, &stems)?;
    let long = query.split_whitespace().count() > 40;
    let mut weights: Vec<(String, f64)> = vec![];
    for (raw, distinct) in &candidates {
        let Some(stem) = stems.get(raw) else { continue };
        let word = repaired.get(raw).unwrap_or(raw);
        let count = *df.get(stem).unwrap_or(&0.0);
        if count == 0.0 || (total > 1000.0 && count > total * 0.2) {
            continue;
        }
        let (catalog, projects) = *domain.get(stem).unwrap_or(&(0.0, 0.0));
        if (ulen(word) == 2 && catalog == 0.0) || (long && catalog == 0.0 && projects == 0.0) {
            continue;
        }
        let mut idf = (1.0 + total / count).ln();
        if catalog + projects < 3.0 && !distinct {
            idf *= 0.5;
        }
        let weight = idf * if *distinct { 1.5 } else { 1.0 } * (1.0 + catalog.ln_1p() / count.ln_1p());
        if let Some(entry) = weights.iter_mut().find(|(w, _)| w == word) {
            entry.1 = weight;
        } else {
            weights.push((word.clone(), weight));
        }
    }
    if weights.is_empty() {
        return Ok((vec![], vec![]));
    }
    let weight = |word: &str| weights.iter().find(|(w, _)| w == word).map(|(_, v)| *v).unwrap_or(0.0);
    let mut queries = vec![weights.iter().map(|(w, _)| w.clone()).collect::<Vec<_>>()];
    for clause in clauses(query) {
        let words: Vec<_> = raw_terms(&clause).into_iter().map(|(w, _)| repaired.get(&w).cloned().unwrap_or(w)).filter(|w| weight(w) > 0.0).collect();
        if words.len() >= 2 && words != queries[0] {
            queries.push(words);
        }
    }
    queries.truncate(index.bound.terms.map_or(usize::MAX, |_| QUERIES));
    let mut fused: Vec<(String, f64)> = vec![];
    let mut families = HashMap::new();
    for (n, words) in queries.iter().enumerate() {
        if n > 0 && index.bound.expired() {
            break;
        }
        let mut top = words.clone();
        top.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
        top.truncate(16);
        let q = top.iter().map(|w| match_term(w)).collect::<Vec<_>>().join(" OR ");
        let mut counts = HashMap::<String, usize>::new();
        for (id, family, _) in index.ranked(&q, None, as_of, CAP)? {
            if !FAMILIES.contains(&family.as_str()) {
                continue;
            }
            let rank = counts.entry(family.clone()).or_default();
            let gain = 1.0 / (40 + *rank) as f64;
            *rank += 1;
            families.insert(id.clone(), family);
            if let Some((_, v)) = fused.iter_mut().find(|(i, _)| *i == id) {
                *v += gain;
            } else {
                fused.push((id, gain));
            }
        }
    }
    let qwords: Vec<_> = weights.iter().map(|(w, _)| w.clone()).collect();
    let q = qwords.iter().map(|w| match_term(w)).collect::<Vec<_>>().join(" OR ");
    let generic = ["skill", "main", "index", "notes", "readme", "agents", "test", "lib", "references", "brief", "status"];
    let titled = if index.bound.expired() { vec![] } else { index.ranked(&q, Some(Field::Name), as_of, 400)? };
    for (id, family, reference) in titled {
        let reference = reference.unwrap_or_default();
        let base = reference.split('#').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_lowercase();
        let base = re(r"\.[a-z]+$").replace(&base, "");
        let parts: Vec<_> = base.split(|c: char| !c.is_ascii_alphanumeric()).filter(|p| p.len() > 2 && !generic.contains(p)).map(str::to_owned).collect();
        let names = if parts.is_empty() { reference.split('/').rev().nth(1).unwrap_or("").to_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).map(str::to_owned).collect() } else { parts };
        if !names.is_empty() && names.iter().all(|p| qwords.iter().any(|w| p.starts_with(w) || w.starts_with(p))) {
            families.insert(id.clone(), family);
            if let Some((_, v)) = fused.iter_mut().find(|(i, _)| *i == id) {
                *v += 0.05;
            } else {
                fused.push((id, 0.05));
            }
        }
    }
    let mut ranked = fused.clone();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut ids = Vec::new();
    for rank in 0..500 {
        let next: Vec<_> = FAMILIES.iter().filter_map(|f| ranked.iter().filter(|(id, _)| families.get(id).is_some_and(|v| v == f)).nth(rank).map(|(id, _)| id.clone())).collect();
        if next.is_empty() {
            break;
        }
        ids.extend(next.into_iter().take(500 - ids.len()));
        if ids.len() >= 500 {
            break;
        }
    }
    if ids.is_empty() {
        return Ok((vec![], qwords));
    }
    let top_fused = fused.iter().map(|(_, v)| *v).fold(0.0, f64::max);
    let mut hit_families = HashMap::new();
    let mut out = Vec::new();
    for id in &ids {
        let Some(doc) = index.doc(&Term::string(id))? else { continue };
        let hay = format!("{}\n{}", doc.title, doc.text).to_lowercase();
        let mut matched: Vec<_> = qwords.iter().filter(|w| hay.contains(&if ulen(w) > 5 { cut(w, ulen(w) - 2) } else { (*w).clone() })).cloned().collect();
        matched.sort();
        let cover = queries
            .iter()
            .map(|words| {
                let mass: f64 = words.iter().map(|w| weight(w)).sum();
                words.iter().filter(|w| matched.contains(w)).map(|w| weight(w)).sum::<f64>() / mass
            })
            .fold(0.0, f64::max);
        let decay = if doc.decays { 0.5f64.powf(((now - doc.at) / 86400.0).max(0.0) / 7.0) } else { 1.0 };
        let score = (fused.iter().find(|(i, _)| i == id).map_or(0.0, |(_, v)| *v) / top_fused + cover) * (0.5 + 0.5 * decay);
        hit_families.insert(format!("{}\0{}", doc.source, doc.id), families[id].clone());
        out.push(Hit { doc, cover, score, matched });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut retained = HashSet::new();
    for family in FAMILIES {
        for (i, _) in out.iter().enumerate().filter(|(_, h)| hit_families.get(&format!("{}\0{}", h.source, h.id)).is_some_and(|f| f == family)).take(limit / 8) {
            retained.insert(i);
        }
    }
    for i in 0..out.len() {
        if retained.len() >= limit {
            break;
        }
        retained.insert(i);
    }
    let mut words = weights.clone();
    words.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok((out.into_iter().enumerate().filter_map(|(i, h)| retained.contains(&i).then_some(h)).collect(), words.into_iter().map(|(w, _)| w).collect()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
