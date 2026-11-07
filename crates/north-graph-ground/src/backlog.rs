//! The ready set of north's backlog: each unticked box of an open issue with no live wait on an unticked box or an
//! open issue, and each box that does wait, with the edge that holds it. It reads what `north graph ingest` told
//! (`waits-on`, `doc-waits-on`, `blocked-by`, `follows`, `rank`, `label`, `loss-usd`, `box-usd`), so a dependency and
//! an order live in the graph once and dispatch, the stop guard, the proxy's launch guard and `north graph ready` agree.
//! The ready boxes run in measured order: the 24 h harness usage an issue owns per dollar one of its boxes costs, most
//! first, with priority label, then backlog-ranking.md's rank, breaking ties (north#734).
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term, default_path, parse_query};
use serde_json::Value;

use crate::rules::{held_query, label_query, rank_query, ready_query, worth_query};
use crate::show::name;

const PATIENCE: Duration = Duration::from_millis(400);
const PRIORITIES: [&str; 4] = ["priority:now", "-", "priority:next", "priority:later"];

/// Where a priority label puts an issue among equals, `priority:now` first and an unlabelled issue second.
pub fn priority<'a>(labels: impl IntoIterator<Item = &'a str>) -> usize {
    let labels: Vec<&str> = labels.into_iter().collect();
    PRIORITIES.iter().position(|priority| labels.contains(priority)).unwrap_or(1)
}

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

/// The 24 h harness usage an issue owns and what one of its boxes costs, as the ledger measured them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Worth {
    pub loss_usd: f64,
    pub box_usd: f64,
}

impl Worth {
    /// Dollars of measured loss a fix removes per dollar of a box.
    pub fn ratio(self) -> f64 {
        if self.box_usd > 0.0 { self.loss_usd / self.box_usd } else { 0.0 }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Backlog {
    pub ranks: BTreeMap<String, i64>,
    pub worth: BTreeMap<String, Worth>,
    pub priorities: BTreeMap<String, usize>,
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

fn float(term: &Term) -> Option<f64> {
    match term {
        Term::Atom(Atom::Float(value)) => Some(value.value()),
        Term::Atom(Atom::Int(value)) => Some(*value as f64),
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
    let ranks = ask(graph, &rank_query())?.iter().filter_map(|row| Some((string(row.first()?), integer(row.get(1)?)?))).collect();
    let worth = ask(graph, &worth_query())?.iter().filter_map(|row| Some((string(row.first()?), Worth { loss_usd: float(row.get(1)?)?, box_usd: float(row.get(2)?)? }))).collect();
    let mut labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in ask(graph, &label_query())? {
        if let (Some(issue), Some(label)) = (row.first(), row.get(1)) {
            labels.entry(string(issue)).or_default().push(string(label));
        }
    }
    let priorities = labels.into_iter().map(|(issue, labels)| (issue, priority(labels.iter().map(String::as_str)))).collect();
    let ready = ask(graph, &ready_query())?
        .iter()
        .filter_map(|row| Some(Entry { issue: string(row.get(1)?), node: string(row.get(2)?), text: string(row.get(3)?) }))
        .collect();
    let held = ask(graph, &held_query())?
        .iter()
        .filter_map(|row| Some(Held { entry: Entry { issue: string(row.first()?), node: string(row.get(1)?), text: string(row.get(2)?) }, edge: string(row.get(3)?), edge_text: string(row.get(4)?) }))
        .collect();
    let mut backlog = Backlog { ranks, worth, priorities, ready, held };
    backlog.order();
    Ok(backlog)
}

fn product(text: &str) -> bool {
    !text.starts_with("Clean-up:")
}

impl Backlog {
    pub fn rank(&self, issue: &str) -> Option<i64> {
        self.ranks.get(issue).copied()
    }

    pub fn worth(&self, issue: &str) -> Worth {
        self.worth.get(issue).copied().unwrap_or_default()
    }

    /// Sorts the ready boxes most measured worth first, then by priority label and rank; a sort is stable, so the
    /// query's order holds among equals.
    fn order(&mut self) {
        let mut ready = std::mem::take(&mut self.ready);
        let priority = |entry: &Entry| self.priorities.get(&entry.issue).copied().unwrap_or(1);
        let rank = |entry: &Entry| self.rank(&entry.issue).unwrap_or(i64::MAX);
        ready.sort_by(|a, b| self.worth(&b.issue).ratio().total_cmp(&self.worth(&a.issue).ratio()).then(priority(a).cmp(&priority(b))).then(rank(a).cmp(&rank(b))));
        self.ready = ready;
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

    /// The measured figure an issue ranks by, then its rank in backlog-ranking.md.
    pub fn standing(&self, issue: &str) -> String {
        let worth = self.worth(issue);
        let measured = if worth.loss_usd > 0.0 { format!("{:.1} = ${:.2} lost 24 h / ${:.2} a box", worth.ratio(), worth.loss_usd, worth.box_usd) } else { "0 measured".to_owned() };
        let rank = self.rank(issue).map_or_else(|| "unranked".to_owned(), |rank| format!("rank {rank}"));
        format!("{measured}, {rank}")
    }

    fn shown(&self, entry: &Entry) -> bool {
        let same_issue_product = |other: &Entry| other.issue == entry.issue && product(&other.text);
        product(&entry.text) || !self.ready.iter().chain(self.held.iter().map(|held| &held.entry)).any(same_issue_product)
    }

    /// The issues with a ready box, in the order their boxes run.
    pub fn leaders(&self) -> Vec<&str> {
        let mut issues: Vec<&str> = Vec::new();
        for entry in self.ready.iter().filter(|entry| self.shown(entry)) {
            if !issues.contains(&entry.issue.as_str()) {
                issues.push(&entry.issue);
            }
        }
        issues
    }

    /// The ready boxes in measured order, then each held box with its edge; a Clean-up box is listed only for an issue with no other box open.
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
