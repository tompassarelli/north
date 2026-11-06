//! The ready set of north's backlog: each unticked box of an open issue, in rank order, with no live
//! wait on an unticked box or an open issue, and each box that does wait, with the edge that holds it.
//! It reads what `north graph ingest` told (`waits-on`, `doc-waits-on`, `blocked-by`, `follows`, `rank`),
//! so a dependency lives in the graph once and dispatch, the stop guard and `north graph ready` agree.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term, default_path, parse_query};
use serde_json::Value;

use crate::rules::{held_query, rank_query, ready_query};
use crate::show::name;

const PATIENCE: Duration = Duration::from_millis(400);


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub issue: String,
    pub node: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Held {
    pub entry: Entry,
    pub edge: String,
    pub edge_text: String,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Backlog {
    pub ranks: BTreeMap<String, i64>,
    pub ready: Vec<Entry>,
    pub held: Vec<Held>,
}

pub enum Verdict {
    Unknown,
    Ready,
    Held(String),
}

fn integer(term: &Term) -> Option<i64> {
    match term {
        Term::Atom(Atom::Int(value)) => Some(*value),
        _ => None,
    }
}

fn string(term: &Term) -> String {
    name(term).unwrap_or_default().to_owned()
}

pub fn read(db: &Path) -> Result<Backlog, String> {
    let graph = Graph::open_read_only(db, PATIENCE).map_err(|why| why.to_string())?;
    read_from(&graph)
}

pub fn read_default() -> Result<Backlog, String> {
    read(&default_path())
}

fn ask(graph: &Graph, query: &Value) -> Result<Vec<Vec<Term>>, String> {
    let query = parse_query(&query.to_string()).map_err(|why| why.to_string())?;
    graph.ask(&query).map(|answer| answer.rows).map_err(|why| why.to_string())
}

pub fn read_from(graph: &Graph) -> Result<Backlog, String> {
    let ranks: BTreeMap<String, i64> = ask(graph, &rank_query())?.iter().filter_map(|row| Some((string(row.first()?), integer(row.get(1)?)?))).collect();
    let ready = ask(graph, &ready_query())?
        .iter()
        .filter_map(|row| Some(Entry { issue: string(row.get(1)?), node: string(row.get(2)?), text: string(row.get(3)?) }))
        .collect();
    let held = ask(graph, &held_query())?
        .iter()
        .filter_map(|row| Some(Held { entry: Entry { issue: string(row.first()?), node: string(row.get(1)?), text: string(row.get(2)?) }, edge: string(row.get(3)?), edge_text: string(row.get(4)?) }))
        .collect();
    Ok(Backlog { ready, held, ranks })
}

fn product(text: &str) -> bool {
    !text.starts_with("Clean-up:")
}

impl Backlog {
    pub fn rank(&self, issue: &str) -> Option<i64> {
        self.ranks.get(issue).copied()
    }

    pub fn verdict(&self, issue: &str) -> Verdict {
        let ready: Vec<&Entry> = self.ready.iter().filter(|entry| entry.issue == issue).collect();
        let held: Vec<&Held> = self.held.iter().filter(|held| held.entry.issue == issue).collect();
        if ready.iter().any(|entry| product(&entry.text)) || (held.iter().all(|held| !product(&held.entry.text)) && !ready.is_empty()) {
            return Verdict::Ready;
        }
        match held.iter().find(|held| product(&held.entry.text)).or_else(|| held.first()) {
            Some(held) => Verdict::Held(format!("box \"{}\" waits on {} \"{}\"", cut(&held.entry.text, 60), held.edge, cut(&held.edge_text, 60))),
            None => Verdict::Unknown,
        }
    }

    fn standing(&self, issue: &str) -> String {
        self.rank(issue).map_or_else(|| "unranked".to_owned(), |rank| format!("rank {rank}"))
    }

    fn shown(&self, entry: &Entry) -> bool {
        let same_issue_product = |other: &Entry| other.issue == entry.issue && product(&other.text);
        product(&entry.text) || !self.ready.iter().chain(self.held.iter().map(|held| &held.entry)).any(same_issue_product)
    }

    /// The ready boxes in rank order, then each held box with its edge; a Clean-up box is listed only for an issue with no other box open.
    pub fn lines(&self) -> Vec<String> {
        let ready = self.ready.iter().filter(|entry| self.shown(entry)).enumerate().map(|(place, entry)| format!("ready {}\t{}\t{}\t{}", place + 1, entry.issue, self.standing(&entry.issue), cut(&entry.text, 100)));
        let held = self.held.iter().filter(|held| self.shown(&held.entry)).map(|held| format!("held\t{}\t{}\t{}\twaits on {} \"{}\"", held.entry.issue, self.standing(&held.entry.issue), cut(&held.entry.text, 100), held.edge, cut(&held.edge_text, 60)));
        ready.chain(held).collect()
    }
}

fn cut(text: &str, limit: usize) -> String {
    let text = text.split(" (owner").next().unwrap_or(text);
    if text.chars().count() <= limit { text.to_owned() } else { format!("{}…", text.chars().take(limit).collect::<String>()) }
}
