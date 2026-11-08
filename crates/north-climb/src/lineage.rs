//! The climb archive in north-graph: each candidate a `climb:ID` node told under writer `climb` in one
//! transaction per run, scored only from kept boxes the `corroborated` rule holds by `checker` as the
//! graph stood at the node's landing seq, and read back so parents are drawn across the whole archive.
use std::collections::{BTreeMap, HashSet};

use north_base::platform::harness;
use north_dispatch::ledger::closed::ArmBox;
use north_dispatch::threads::claims::box_node;
use north_graph::{Action, Atom, Graph, GraphError, Term, Triple, parse_query};

use crate::candidate::Candidate;
use crate::loss::Loss;
use crate::reward::Reward;

pub const WRITER: &str = "climb";
pub const ROOT: &str = "climb:current";
const LINEAGE: &str = "harness/graph-rules/lineage.json";
const CORROBORATED: &str = "harness/graph-rules/corroborated.json";
const CHECKER: &str = "checker";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Proposed,
    Refused,
    Arm,
    Kept,
    Reverted,
}

impl Status {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Refused => "refused",
            Self::Arm => "arm",
            Self::Kept => "kept",
            Self::Reverted => "reverted",
        }
    }

    fn ran(status: &str) -> bool {
        status == Self::Arm.keyword() || status == Self::Kept.keyword()
    }
}

/// One run's candidate as the archive records it.
pub struct Entry<'a> {
    pub candidate: &'a Candidate,
    pub loss: &'a Loss,
    pub status: Status,
    pub replay_refusals: usize,
    pub replay_reached: usize,
    pub reward: Option<Reward>,
}

/// A node read back from the archive.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub parent: String,
    pub status: String,
    pub landed_seq: i64,
    pub scored: Option<f64>,
}

pub fn node(id: &str, seq: i64) -> String {
    format!("climb:{id}-{seq}")
}

/// Tells `entry` as `climb:ID-SEQ` facts in one transaction, `seq` being the head it was judged at.
pub fn record(graph: &mut Graph, entry: &Entry, seq: i64) -> Result<String, GraphError> {
    let Entry { candidate, loss, status, replay_refusals, replay_reached, reward } = entry;
    let id = node(&candidate.id, seq);
    let count = |value: usize| Term::int(i64::try_from(value).unwrap_or(i64::MAX));
    let mut facts = vec![
        ("parent", Term::string(&candidate.parent)),
        ("change", Term::string(candidate.change.describe())),
        ("loss-category", Term::string(&loss.category)),
        ("loss-usd", Term::float(loss.usd())),
        ("replay-refusals", count(*replay_refusals)),
        ("replay-reached", count(*replay_reached)),
        ("status", Term::keyword(status.keyword())),
        ("landed-seq", Term::int(seq)),
    ];
    facts.extend(candidate.change.touches().into_iter().map(|file| ("mutates", Term::string(file))));
    facts.extend(candidate.change.arm().map(|arm| ("arm", Term::string(arm))));
    if let Some(reward) = reward {
        facts.push(("corroborated-kept", count(reward.verified)));
        facts.extend((reward.verified > 0).then(|| ("scored", Term::float(reward.usd / reward.verified as f64))));
    }
    let actions: Vec<Action> = facts.into_iter().map(|(property, value)| Action::assert(Triple::new(Term::string(&id), Term::keyword(property), value))).collect();
    graph.apply(WRITER, &actions)?;
    Ok(id)
}

/// Every node `climb` has told, oldest landing seq first.
pub fn archive(graph: &Graph) -> Result<Vec<Node>, GraphError> {
    let query = parse_query(harness::table(LINEAGE)).map_err(|why| GraphError::Invalid(why.to_string()))?;
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for row in graph.ask(&query)?.rows {
        let [Term::Atom(Atom::String(id)), Term::Atom(Atom::Keyword(property)), value] = &row[..] else { continue };
        let node = nodes.entry(id.clone()).or_insert_with(|| Node { id: id.clone(), parent: String::new(), status: String::new(), landed_seq: 0, scored: None });
        match (property.as_str(), value) {
            ("parent", Term::Atom(Atom::String(parent))) => node.parent.clone_from(parent),
            ("status", Term::Atom(Atom::Keyword(status))) => node.status.clone_from(status),
            ("landed-seq", Term::Atom(Atom::Int(seq))) => node.landed_seq = *seq,
            ("scored", Term::Atom(Atom::Float(usd))) => node.scored = Some(usd.value()),
            _ => {}
        }
    }
    let mut nodes: Vec<Node> = nodes.into_values().collect();
    nodes.sort_by_key(|node| node.landed_seq);
    Ok(nodes)
}

/// Draws a parent across the archive as the Darwin Gödel Machine does: the root and every node that ran
/// as an arm or was kept, each weighted by its score against the best and by one over one plus its
/// children, so a strong node is likely and a node already built on often is less so. `seed` fixes the draw.
pub fn draw(archive: &[Node], seed: u64) -> String {
    let children = |id: &str| archive.iter().filter(|node| node.parent == id).count();
    let best = archive.iter().filter_map(|node| node.scored).fold(f64::INFINITY, f64::min);
    let performance = |scored: Option<f64>| scored.filter(|usd| *usd > 0.0).map_or(0.5, |usd| best / usd);
    let mut eligible = vec![(ROOT.to_owned(), performance(None) / (1 + children(ROOT)) as f64)];
    eligible.extend(archive.iter().filter(|node| Status::ran(&node.status)).map(|node| (node.id.clone(), performance(node.scored) / (1 + children(&node.id)) as f64)));
    let total: f64 = eligible.iter().map(|(_, weight)| weight).sum();
    let mut point = unit(seed) * total;
    for (id, weight) in &eligible {
        if point < *weight {
            return id.clone();
        }
        point -= weight;
    }
    eligible.last().map_or_else(|| ROOT.to_owned(), |(id, _)| id.clone())
}

fn unit(seed: u64) -> f64 {
    let mut mixed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (mixed ^ (mixed >> 31)) as f64 / (u64::MAX as f64 + 1.0)
}

pub fn seed(text: &str, seq: i64) -> u64 {
    text.bytes().chain(seq.to_le_bytes()).fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// The box nodes the `corroborated` rule holds by `checker` in the graph as it stood at `seq`.
pub fn corroborated(graph: &Graph, seq: i64) -> Result<HashSet<String>, GraphError> {
    let mut rule: serde_json::Value = serde_json::from_str(harness::table(CORROBORATED)).map_err(|why| GraphError::Invalid(why.to_string()))?;
    rule["asOf"] = seq.into();
    let query = parse_query(&rule.to_string()).map_err(|why| GraphError::Invalid(why.to_string()))?;
    let string = |term: Option<&Term>| match term {
        Some(Term::Atom(Atom::String(value))) => Some(value.clone()),
        _ => None,
    };
    Ok(graph.ask(&query)?.rows.iter().filter(|row| string(row.get(2)).as_deref() == Some(CHECKER)).filter_map(|row| string(row.first())).collect())
}

/// What `arm` of `category` earned over `boxes`, counting a kept box verified only when `corroborated` names it.
pub fn reward(boxes: &[ArmBox], category: &str, arm: &str, corroborated: &HashSet<String>, judge: u32) -> Reward {
    let ran: Vec<&ArmBox> = boxes.iter().filter(|armed| armed.category == category && armed.arm == arm).collect();
    let kept: Vec<&&ArmBox> = ran.iter().filter(|armed| armed.closed && !armed.reworked).collect();
    let verified = kept.iter().filter(|armed| armed.box_text.as_deref().and_then(|text| box_node(&armed.item, text)).is_some_and(|node| corroborated.contains(&node))).count();
    Reward { judge, usd: ran.iter().map(|armed| armed.usd).sum(), kept: kept.len(), verified }
}
