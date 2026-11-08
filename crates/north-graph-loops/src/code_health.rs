use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use north_graph::{Graph, Term, Triple};

use crate::cells;
use crate::rules::{ask, open_read_only, open_writable, tell_set, ticks_from};

pub const WRITER: &str = "code-health";
const RULE: &str = "harness/graph-rules/code-health-review.json";
const HAND_FILES: &str = "code-health/hand-files-median";
const REVERT_SHARE: &str = "code-health/revert-share";
const REASON: &str = "code-health/reason";

/// What the last `code-health signal --record` of a repository measured since its last review:
/// `(review:REPO :code-health/hand-files-median F)`, `:code-health/revert-share F` and one
/// `:code-health/reason TEXT` per reason that fired.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Review {
    pub hand_files_median: Option<f64>,
    pub revert_share: Option<f64>,
    pub reasons: Vec<String>,
}

/// Every repository's review reading and the instant the writer last ticked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Told {
    pub reviews: BTreeMap<String, Review>,
    pub at: Option<i64>,
}

fn subject(repo: &str) -> Term {
    Term::string(format!("review:{repo}"))
}

fn facts(repo: &str, review: &Review) -> BTreeSet<Triple> {
    let on = |predicate: &str, object: Term| Triple::new(subject(repo), Term::keyword(predicate), object);
    let figures = [(HAND_FILES, review.hand_files_median), (REVERT_SHARE, review.revert_share)];
    let reasons = review.reasons.iter().map(|reason| on(REASON, Term::string(reason.clone())));
    figures.into_iter().filter_map(|(predicate, figure)| Some(on(predicate, Term::float(figure?)))).chain(reasons).collect()
}

/// The reviews the writer told, through harness/graph-rules/code-health-review.json, and its newest tick.
pub fn read_from(graph: &Graph) -> Result<Told, String> {
    let mut reviews: BTreeMap<String, Review> = BTreeMap::new();
    for row in ask(graph, RULE)? {
        let (Some(repo), Some(Term::Atom(north_graph::Atom::Keyword(predicate))), Some(value)) = (row.first().and_then(cells::text).and_then(|subject| subject.strip_prefix("review:")), row.get(1), row.get(2).and_then(cells::value)) else { continue };
        let review = reviews.entry(repo.to_owned()).or_default();
        match (predicate.as_str(), value.as_f64(), value.as_str()) {
            (HAND_FILES, Some(figure), _) => review.hand_files_median = Some(figure),
            (REVERT_SHARE, Some(figure), _) => review.revert_share = Some(figure),
            (REASON, _, Some(reason)) => review.reasons.push(reason.to_owned()),
            _ => {}
        }
    }
    reviews.values_mut().for_each(|review| review.reasons.sort());
    Ok(Told { reviews, at: ticks_from(graph)?.get(WRITER).copied() })
}

pub fn read(db: &Path) -> Result<Told, String> {
    read_from(&open_read_only(db)?)
}

/// Tells one recording of `repo` as one transaction: the reading the graph held for it becomes
/// `review`, and the writer's tick moves to `at`.
pub fn tell_to(graph: &mut Graph, repo: &str, review: &Review, at: i64) -> Result<(), String> {
    let prior = read_from(graph)?;
    let held = prior.reviews.get(repo).map(|held| facts(repo, held)).unwrap_or_default();
    tell_set(graph, WRITER, &held, &facts(repo, review), at)
}

pub fn tell(db: &Path, repo: &str, review: &Review, at: i64) -> Result<(), String> {
    tell_to(&mut open_writable(db)?, repo, review, at)
}
