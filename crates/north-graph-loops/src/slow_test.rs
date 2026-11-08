use std::collections::BTreeSet;
use std::path::Path;

use north_graph::{Graph, Term, Triple};

use crate::cells;
use crate::rules::{self, PATIENCE};

pub const WRITER: &str = "flow";
const RULE: &str = "harness/graph-rules/slow-test.json";
const PASS_S: &str = "test/pass-s";
const SLOW: &str = "test/slow";

/// A test over its line in each of its newest land check runs, and how long its newest run took.
#[derive(Clone, Debug, PartialEq)]
pub struct Slow {
    pub binary: String,
    pub name: String,
    pub seconds: f64,
}

fn triples(slow: &Slow) -> [Triple; 2] {
    let subject = || Term::string(format!("test:{} {}", slow.binary, slow.name));
    [Triple::new(subject(), Term::keyword(PASS_S), Term::float(slow.seconds)), Triple::new(subject(), Term::keyword(SLOW), Term::boolean(true))]
}

pub fn read_from(graph: &Graph) -> Result<Vec<Slow>, String> {
    Ok(rules::ask(graph, RULE)?
        .iter()
        .filter_map(|row| {
            let (binary, name) = row.first().and_then(cells::text)?.strip_prefix("test:")?.split_once(' ')?;
            Some(Slow { binary: binary.to_owned(), name: name.to_owned(), seconds: cells::value(row.get(1)?)?.as_f64()? })
        })
        .collect())
}

pub fn read(db: &Path) -> Result<Vec<Slow>, String> {
    read_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?)
}

pub fn tell_to(graph: &mut Graph, slow: &[Slow], at: i64) -> Result<(), String> {
    let set = |slow: &[Slow]| slow.iter().flat_map(triples).collect::<BTreeSet<_>>();
    rules::tell_set(graph, WRITER, &set(&read_from(graph)?), &set(slow), at)
}

/// Tells one flow detector run: each test over its line in its newest runs, and the tick.
pub fn tell(db: &Path, slow: &[Slow], at: i64) -> Result<(), String> {
    tell_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, slow, at)
}
