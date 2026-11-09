use super::*;
use north_graph::{Action, Graph, Term, Triple};
use std::collections::{HashMap, HashSet};
use store::{Doc, WRITER, source_predicate};

const WATERMARKS: &str = "watermarks.json";

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(super) struct Mark {
    pub at: f64,
    pub refreshed: f64,
    pub rows: usize,
}

pub(super) fn marks() -> HashMap<String, Mark> {
    serde_json::from_str(&read(&state_dir().join(WATERMARKS))).unwrap_or_default()
}

fn save(marks: &HashMap<String, Mark>) -> Result<()> {
    let path = state_dir().join(WATERMARKS);
    let staged = path.with_extension("json.tmp");
    fs::write(&staged, serde_json::to_string(marks)?)?;
    fs::rename(staged, path)?;
    Ok(())
}

pub fn refresh(only: &[String], force: bool, wait: bool) -> Result<Value> {
    let Some(lock) = refresh_lock(&state_dir(), wait)? else {
        return Ok(json!({"busy":true}));
    };
    let result = (|| {
        let mut graph = Graph::open(&north_graph::default_path())?;
        let mut marks = marks();
        let mut counts = serde_json::Map::new();
        for (name, complete, seconds, retention) in SOURCES {
            if !only.is_empty() && !only.iter().any(|wanted| wanted == name) {
                continue;
            }
            let started = Timestamp::now().unix_seconds_f64();
            let mark = marks.get(name).copied();
            if !force && started - mark.map_or(0.0, |m| m.refreshed) <= seconds {
                counts.insert(name.into(), json!(format!("fresh {}", mark.map_or(0, |m| m.rows))));
                continue;
            }
            let since = if complete || force || mark.is_none() { None } else { mark.map(|m| m.at) };
            let docs = match iter_docs(name, since) {
                Ok(docs) => docs,
                Err(e) => {
                    eprintln!("grounding: source {name} failed: {e}");
                    continue;
                }
            };
            let newest = docs.iter().map(|d| d.at).fold(mark.map_or(0.0, |m| m.at), f64::max);
            let floor = retention.map(|days| started - days * 86400.0);
            let rows = write(&mut graph, name, docs.iter().map(stored).collect(), since.is_none(), floor)?;
            marks.insert(name.into(), Mark { at: newest, refreshed: started, rows });
            save(&marks)?;
            counts.insert(name.into(), json!(docs.len()));
        }
        Ok(Value::Object(counts))
    })();
    drop(lock);
    result
}

fn write(graph: &mut Graph, source: &str, docs: Vec<Doc>, whole: bool, floor: Option<f64>) -> Result<usize> {
    let mut have: HashMap<Term, HashSet<Triple>> = HashMap::new();
    for told in graph.described(&source_predicate(), &Term::string(source))? {
        if told.writer == WRITER {
            have.entry(told.triple.t1.clone()).or_default().insert(told.triple);
        }
    }
    let mut want: HashMap<Term, Doc> = HashMap::new();
    for doc in docs {
        want.insert(doc.subject(), doc);
    }
    let expired = |doc: &Doc| floor.is_some_and(|floor| doc.at < floor);
    let mut actions = Vec::new();
    let mut rows = 0;
    for (subject, facts) in &have {
        let kept = match want.get(subject) {
            Some(doc) if !expired(doc) => doc.facts().into_iter().collect(),
            Some(_) => HashSet::new(),
            None if whole => HashSet::new(),
            None => {
                let doc = Doc::from_facts(subject, facts);
                if doc.as_ref().is_some_and(expired) { HashSet::new() } else { facts.clone() }
            }
        };
        actions.extend(facts.difference(&kept).cloned().map(Action::retract));
        actions.extend(kept.difference(facts).cloned().map(Action::assert));
        rows += usize::from(!kept.is_empty());
    }
    for (subject, doc) in &want {
        if !have.contains_key(subject) && !expired(doc) {
            actions.extend(doc.facts().into_iter().map(Action::assert));
            rows += 1;
        }
    }
    graph.apply(WRITER, &actions)?;
    Ok(rows)
}

fn stored(d: &Doc) -> Doc {
    Doc { title: redact(&d.title), text: redact(&cut(&d.text, 4000)), ..d.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, text: &str, at: f64) -> Doc {
        Doc { source: "policy".into(), id: id.into(), reference: format!("north:{id}"), title: format!("rule {id}"), text: text.into(), at, decays: false }
    }

    fn occurrences(graph: &Graph) -> u64 {
        graph.validate().unwrap().occurrences
    }

    #[test]
    fn a_refresh_writes_only_the_propositions_that_changed() {
        let mut graph = Graph::open_in_memory().unwrap();
        assert_eq!(write(&mut graph, "policy", vec![doc("a", "landing queue", 1.0), doc("b", "lander probes", 2.0)], true, None).unwrap(), 2);
        let first = occurrences(&graph);
        assert_eq!(write(&mut graph, "policy", vec![doc("a", "landing queue", 1.0), doc("b", "lander probes", 2.0)], true, None).unwrap(), 2);
        assert_eq!(occurrences(&graph), first, "an unchanged refresh adds no occurrence");
        assert_eq!(write(&mut graph, "policy", vec![doc("a", "landing queue p90", 1.0)], true, None).unwrap(), 1);
        assert_eq!(occurrences(&graph), first + 2 + 6, "the changed text is one retraction and one assertion, the dropped doc six retractions");
        let docs = graph.described(&source_predicate(), &Term::string("policy")).unwrap();
        let facts: Vec<Triple> = docs.into_iter().map(|told| told.triple).collect();
        assert_eq!(Doc::from_facts(&store::subject("policy", "a"), &facts), Some(doc("a", "landing queue p90", 1.0)));
        assert_eq!(write(&mut graph, "policy", vec![doc("c", "new", 3.0)], false, Some(0.5)).unwrap(), 2, "an incremental refresh keeps the docs it did not read");
    }
}
