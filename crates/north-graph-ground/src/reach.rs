use std::path::Path;
use std::time::{Duration, Instant};

use north_graph::{Graph, GraphError, Term, Told, default_path};

use crate::seed::{Seed, seeds};
use crate::{file, issue};

const PATIENCE: Duration = Duration::from_millis(40);
const MENTION_LIMIT: usize = 400;
const CHARS: usize = 1500;

pub struct Reach<'g> {
    graph: &'g Graph,
    until: Option<Instant>,
}

impl<'g> Reach<'g> {
    pub fn new(graph: &'g Graph, until: Option<Instant>) -> Self {
        Self { graph, until }
    }

    pub fn about(&self, term: &Term) -> Vec<Told> {
        if self.late() {
            return Vec::new();
        }
        self.graph.mentions(term, MENTION_LIMIT).unwrap_or_default()
    }

    pub fn late(&self) -> bool {
        self.until.is_some_and(|until| Instant::now() >= until)
    }
}

pub fn ground(text: &str, until: Option<Instant>) -> Vec<String> {
    let wanted = seeds(text);
    if wanted.is_empty() {
        return Vec::new();
    }
    ground_with(&default_path(), &wanted, until)
}

pub fn ground_with(db: &Path, wanted: &[Seed], until: Option<Instant>) -> Vec<String> {
    match Graph::open_read_only(db, PATIENCE) {
        Ok(graph) => read(&Reach::new(&graph, until), wanted),
        Err(GraphError::Storage(_)) => Vec::new(),
        Err(why) => vec![format!("Graph unavailable: {why}")],
    }
}

fn read(reach: &Reach, wanted: &[Seed]) -> Vec<String> {
    let share = CHARS / wanted.len().max(1);
    wanted
        .iter()
        .flat_map(|seed| {
            let mut size = 0;
            let lines = match seed {
                Seed::Issue(name) => issue::lines(reach, name),
                Seed::File(path) => file::lines(reach, path),
            };
            lines.into_iter().take_while(move |line| {
                size += line.chars().count() + 1;
                size <= share
            })
        })
        .collect()
}
