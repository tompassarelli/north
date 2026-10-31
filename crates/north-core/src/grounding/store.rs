use super::*;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
pub struct Sqlite {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Sqlite {
    pub fn open(path: &Path, readonly: bool) -> Result<Self> {
        if readonly && !path.exists() {
            return Err(format!("no index at {}", path.display()).into());
        }
        let mut cmd = Command::new("sqlite3");
        if readonly {
            cmd.arg("-readonly");
        }
        let mut child = cmd
            .args(["-json", "-batch"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut db = Self {
            child,
            input,
            output,
        };
        db.exec(if readonly {
            ".bail on\n.timeout 0"
        } else {
            ".bail on\n.timeout 30000"
        })?;
        Ok(db)
    }
    fn request(&mut self, sql: &str) -> Result<String> {
        writeln!(self.input, "{}\n.print __north_grounding_end__", sql)?;
        self.input.flush()?;
        let mut out = String::new();
        loop {
            let mut line = String::new();
            if self.output.read_line(&mut line)? == 0 {
                let mut err = String::new();
                if let Some(mut stream) = self.child.stderr.take() {
                    stream.read_to_string(&mut err)?;
                }
                return Err(err.into());
            }
            if line.trim_end() == "__north_grounding_end__" {
                break;
            }
            out.push_str(&line);
        }
        Ok(out)
    }
    pub fn query(&mut self, sql: &str) -> Result<Vec<Value>> {
        let out = self.request(sql)?;
        if out.trim().is_empty() {
            Ok(Vec::new())
        } else {
            Ok(serde_json::from_str(&out)?)
        }
    }
    pub fn exec(&mut self, sql: &str) -> Result<()> {
        self.request(sql).map(|_| ())
    }
}
impl Drop for Sqlite {
    fn drop(&mut self) {
        let _ = writeln!(self.input, ".quit");
        let _ = self.input.flush();
        let _ = self.child.wait();
    }
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\0', "").replace('\'', "''"))
}
pub use super::common::state_dir;
pub fn db_path() -> PathBuf {
    state_dir().join("index.db")
}
pub fn open(readonly: bool) -> Result<Sqlite> {
    open_at(&db_path(), readonly)
}
pub fn open_at(path: &Path, readonly: bool) -> Result<Sqlite> {
    if !readonly {
        fs::create_dir_all(state_dir())?;
    }
    let mut db = Sqlite::open(path, readonly)?;
    if !readonly {
        db.exec("PRAGMA journal_mode=DELETE; PRAGMA busy_timeout=30000;")?;
        db.exec(SCHEMA)?;
    }
    Ok(db)
}
pub const SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(source UNINDEXED,id UNINDEXED,ref UNINDEXED,title,text,at UNINDEXED,decays UNINDEXED,tokenize='porter unicode61 remove_diacritics 2'); CREATE TABLE IF NOT EXISTS watermarks(source TEXT PRIMARY KEY, at REAL, refreshed REAL, rows INTEGER);";
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
const FAMILY: &str = "case when source='policy' and title like 'handoff %' then 'handoffs' when source='settings' or (source='policy' and ref like '%.toml' and title not like 'rule %' and title not like 'hook %') then 'config' when source='policy' then 'catalog' when source in ('commits','builds') then 'commits/builds' when source in ('threads','github') then 'issues' else source end";
pub fn as_of_filter(as_of: f64) -> String {
    if as_of == 0.0 {
        String::new()
    } else {
        format!(
            " and not ((decays = 1 or source in ('transcripts','builds','ledger','commits','threads','github')) and at > {as_of})"
        )
    }
}
pub fn search(
    query: &str,
    db: &mut Sqlite,
    mut now: f64,
    limit: usize,
    as_of: f64,
) -> Result<(Vec<Hit>, Vec<String>)> {
    if as_of != 0.0 {
        now = now.min(as_of);
    }
    let seen = as_of_filter(as_of);
    let candidates: Vec<_> = raw_terms(query).into_iter().take(60).collect();
    if candidates.is_empty() {
        return Ok((vec![], vec![]));
    }
    let total = db.query("select count(*) n from docs;")?[0]["n"]
        .as_f64()
        .unwrap_or(1.0)
        .max(1.0);
    db.exec(&format!("CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_vocab USING fts5vocab(main, docs, row); CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_instances USING fts5vocab(main, docs, instance); CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_tokens USING fts5(text, tokenize='porter unicode61 remove_diacritics 2'); CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_stems USING fts5vocab(temp, grounding_tokens, instance); DELETE FROM temp.grounding_tokens; INSERT INTO temp.grounding_tokens(text) VALUES ({});", quote(&candidates.iter().map(|(w,_)|w.as_str()).collect::<Vec<_>>().join(" "))))?;
    let mut stems = HashMap::<String, String>::new();
    for row in db.query("select offset, term from temp.grounding_stems;")? {
        if let Some((w, _)) = candidates.get(row["offset"].as_u64().unwrap_or(0) as usize) {
            stems.insert(w.clone(), s(&row, "term"));
        }
    }
    let stats =
        |db: &mut Sqlite, stems: &HashMap<String, String>| -> Result<HashMap<String, f64>> {
            Ok(db
                .query(&format!(
                    "select term, doc from temp.grounding_vocab where term in ({});",
                    stems
                        .values()
                        .map(|s| quote(s))
                        .collect::<Vec<_>>()
                        .join(",")
                ))?
                .into_iter()
                .map(|r| (s(&r, "term"), num(&r, "doc")))
                .collect())
        };
    let mut df = stats(db, &stems)?;
    let mut repaired = HashMap::new();
    for (word, _) in &candidates {
        let Some(stem) = stems.get(word) else {
            continue;
        };
        if df.contains_key(stem) || word.len() < 6 {
            continue;
        }
        let rows=db.query(&format!("select term, doc from temp.grounding_vocab where length(term) between {} and {} and (substr(term,1,2)={} or substr(term,-2)={}) order by doc desc limit 2000;",stem.len().saturating_sub(2),stem.len()+2,quote(&stem[..stem.len().min(2)]),quote(&stem[stem.len().saturating_sub(2)..])))?;
        let mut best = 3;
        for row in rows {
            let term = s(&row, "term");
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
    df = stats(db, &stems)?;
    let domain: HashMap<_,_>=db.query(&format!("select v.term, count(distinct case when d.source!='projects' then v.doc end) catalog, count(distinct case when d.source='projects' then v.doc end) projects from temp.grounding_instances v join docs d on d.rowid=v.doc where v.term in ({}) and (d.source in ('settings','projects') or (d.source='policy' and d.title not like 'handoff %')) group by v.term;",stems.values().map(|s|quote(s)).collect::<Vec<_>>().join(",")))?.into_iter().map(|r|(s(&r,"term"),(num(&r,"catalog"),num(&r,"projects")))).collect();
    let long = query.split_whitespace().count() > 40;
    let mut weights: Vec<(String, f64)> = vec![];
    for (raw, distinct) in &candidates {
        let Some(stem) = stems.get(raw) else {
            continue;
        };
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
        let weight =
            idf * if *distinct { 1.5 } else { 1.0 } * (1.0 + catalog.ln_1p() / count.ln_1p());
        if let Some(entry) = weights.iter_mut().find(|(w, _)| w == word) {
            entry.1 = weight;
        } else {
            weights.push((word.clone(), weight));
        }
    }
    if weights.is_empty() {
        return Ok((vec![], vec![]));
    }
    let weight = |word: &str| {
        weights
            .iter()
            .find(|(w, _)| w == word)
            .map(|(_, v)| *v)
            .unwrap_or(0.0)
    };
    let mut queries = vec![weights.iter().map(|(w, _)| w.clone()).collect::<Vec<_>>()];
    for clause in clauses(query) {
        let words: Vec<_> = raw_terms(&clause)
            .into_iter()
            .map(|(w, _)| repaired.get(&w).cloned().unwrap_or(w))
            .filter(|w| weight(w) > 0.0)
            .collect();
        if words.len() >= 2 && words != queries[0] {
            queries.push(words);
        }
    }
    let mut fused: Vec<(i64, f64)> = vec![];
    let mut families = HashMap::new();
    for words in &queries {
        let mut top = words.clone();
        top.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
        top.truncate(16);
        let q = top
            .iter()
            .map(|w| match_term(w))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut counts = HashMap::<String, usize>::new();
        for row in db.query(&format!("select rowid, {FAMILY} family from docs where docs match {} and rank match 'bm25(0, 0, 0, 4.0, 1.0, 0, 0)'{seen} order by rank;",quote(&q)))? {
            let family=s(&row,"family");let rank=*counts.get(&family).unwrap_or(&0);if rank>=80 || !FAMILIES.contains(&family.as_str()) {continue;}
            counts.insert(family.clone(),rank+1);let id=row["rowid"].as_i64().unwrap_or(0);families.insert(id,family);
            if let Some((_,v))=fused.iter_mut().find(|(i,_)|*i==id){*v+=1.0/(40+rank)as f64;}else{fused.push((id,1.0/(40+rank)as f64));}
            if FAMILIES.iter().all(|f|counts.get(*f)==Some(&80)){break;}
        }
    }
    let qwords: Vec<_> = weights.iter().map(|(w, _)| w.clone()).collect();
    let q = format!(
        "title : ({})",
        qwords
            .iter()
            .map(|w| match_term(w))
            .collect::<Vec<_>>()
            .join(" OR ")
    );
    let generic = [
        "skill",
        "main",
        "index",
        "notes",
        "readme",
        "agents",
        "test",
        "lib",
        "references",
        "brief",
        "status",
    ];
    for row in db.query(&format!("select rowid, ref, {FAMILY} family from docs where docs match {} and source in ('policy','projects'){seen} limit 400;",quote(&q)))? {
        let reference=s(&row,"ref");let base=reference.split('#').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_lowercase();let base=re(r"\.[a-z]+$").replace(&base,"");
        let parts:Vec<_>=base.split(|c:char|!c.is_ascii_alphanumeric()).filter(|p|p.len()>2 && !generic.contains(p)).map(str::to_owned).collect();
        let names=if parts.is_empty(){reference.split('/').rev().nth(1).unwrap_or("").to_lowercase().split(|c:char|!c.is_ascii_alphanumeric()).map(str::to_owned).collect()}else{parts};
        if !names.is_empty() && names.iter().all(|p|qwords.iter().any(|w|p.starts_with(w)||w.starts_with(p))){let id=row["rowid"].as_i64().unwrap_or(0);families.insert(id,s(&row,"family"));if let Some((_,v))=fused.iter_mut().find(|(i,_)|*i==id){*v+=0.05;}else{fused.push((id,0.05));}}
    }
    let mut ranked = fused.clone();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut ids = Vec::new();
    for rank in 0..500 {
        let next: Vec<_> = FAMILIES
            .iter()
            .filter_map(|f| {
                ranked
                    .iter()
                    .filter(|(id, _)| families.get(id).is_some_and(|v| v == f))
                    .nth(rank)
                    .map(|(id, _)| *id)
            })
            .collect();
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
    for row in db.query(&format!(
        "select rowid, source, id, ref, title, text, at, decays from docs where rowid in ({});",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    ))? {
        let id = row["rowid"].as_i64().unwrap_or(0);
        let hay = format!("{}\n{}", s(&row, "title"), s(&row, "text")).to_lowercase();
        let mut matched: Vec<_> = qwords
            .iter()
            .filter(|w| {
                hay.contains(&if ulen(w) > 5 {
                    cut(w, ulen(w) - 2)
                } else {
                    w.to_string()
                })
            })
            .cloned()
            .collect();
        matched.sort();
        let cover = queries
            .iter()
            .map(|words| {
                let mass: f64 = words.iter().map(|w| weight(w)).sum();
                words
                    .iter()
                    .filter(|w| matched.contains(w))
                    .map(|w| weight(w))
                    .sum::<f64>()
                    / mass
            })
            .fold(0.0, f64::max);
        let doc = Doc {
            source: s(&row, "source"),
            id: s(&row, "id"),
            reference: s(&row, "ref"),
            title: s(&row, "title"),
            text: s(&row, "text"),
            at: num(&row, "at"),
            decays: num(&row, "decays") != 0.0,
        };
        let decay = if doc.decays {
            0.5f64.powf(((now - doc.at) / 86400.0).max(0.0) / 7.0)
        } else {
            1.0
        };
        let score = (fused.iter().find(|(i, _)| *i == id).unwrap().1 / top_fused + cover)
            * (0.5 + 0.5 * decay);
        hit_families.insert(format!("{}\0{}", doc.source, doc.id), families[&id].clone());
        out.push(Hit {
            doc,
            cover,
            score,
            matched,
        });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut retained = HashSet::new();
    for family in FAMILIES {
        for (i, _) in out
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                hit_families
                    .get(&format!("{}\0{}", h.source, h.id))
                    .is_some_and(|f| f == family)
            })
            .take(limit / 8)
        {
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
    Ok((
        out.into_iter()
            .enumerate()
            .filter_map(|(i, h)| retained.contains(&i).then_some(h))
            .collect(),
        words.into_iter().map(|(w, _)| w).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_vocabulary_repairs_preserve_coverage_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Sqlite::open(&dir.path().join("index.db"), false).unwrap();
        db.exec(SCHEMA).unwrap();
        db.exec(
            "insert into docs values ('policy','one','one','foobαa quokka','foobαa quokka',0,0);",
        )
        .unwrap();
        let (hits, words) = search("foobaa quokka", &mut db, 0.0, 80, 0.0).unwrap();
        assert_eq!(words, ["foobαa", "quokka"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cover, 1.0);
        assert_eq!(distance("foobaa", "foobαa"), 1);
    }

    #[test]
    fn locked_readonly_index_fails_open_before_the_prompt_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        let mut writer = Sqlite::open(&path, false).unwrap();
        writer.exec(SCHEMA).unwrap();
        writer.exec("BEGIN EXCLUSIVE;").unwrap();
        let started = std::time::Instant::now();
        let mut reader = Sqlite::open(&path, true).unwrap();
        assert!(reader.query("select count(*) from docs;").is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
        writer.exec("ROLLBACK;").unwrap();
    }
}
