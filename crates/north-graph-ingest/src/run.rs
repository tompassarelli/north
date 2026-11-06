use std::collections::HashMap;
use std::path::PathBuf;

use north_graph::{Graph, Synced, Term, default_path};

use crate::places::Places;
use crate::shas::Shas;
use crate::facts::Sourced;
use crate::{corrections, coverage, decisions, issues, ledger, pushes, ranking, touches};

const USAGE: &str = "usage: north graph ingest [--db PATH] [SOURCE...]\nsources: mirror ranking ledger pushes touches decisions corrections coverage (all when none is named)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Mirror,
    Ranking,
    Ledger,
    Pushes,
    Touches,
    Decisions,
    Corrections,
    Coverage,
}

impl Source {
    pub const ALL: [Self; 8] = [Self::Mirror, Self::Ranking, Self::Ledger, Self::Pushes, Self::Touches, Self::Decisions, Self::Corrections, Self::Coverage];

    pub fn name(self) -> &'static str {
        match self {
            Self::Mirror => "mirror",
            Self::Ranking => "ranking",
            Self::Ledger => "ledger",
            Self::Pushes => "pushes",
            Self::Touches => "touches",
            Self::Decisions => "decisions",
            Self::Corrections => "corrections",
            Self::Coverage => "coverage",
        }
    }

    fn read(self, places: &Places, shas: &Shas) -> Result<Sourced, String> {
        match self {
            Self::Mirror => issues::read(places, shas),
            Self::Ranking => ranking::read(places),
            Self::Ledger => ledger::read(places).map(Sourced::from),
            Self::Pushes => pushes::read(places).map(Sourced::from),
            Self::Touches => touches::read(places).map(Sourced::from),
            Self::Decisions => decisions::read(places).map(Sourced::from),
            Self::Corrections => corrections::read(places).map(Sourced::from),
            Self::Coverage => coverage::read(places).map(Sourced::from),
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub failed: bool,
}

pub fn ingest(graph: &mut Graph, places: &Places, sources: &[Source]) -> Report {
    let shas = Shas::read(places);
    let mut report = Report::default();
    let mut replaced: HashMap<Term, &'static str> = HashMap::new();
    for source in sources {
        let outcome = source.read(places, &shas).and_then(|Sourced { telling, notes }| {
            if let Some((predicate, other)) = telling.replaces.iter().find_map(|predicate| replaced.get(predicate).map(|other| (predicate, other))) {
                return Err(format!("it replaces {predicate}, which {other} already replaces in this run"));
            }
            replaced.extend(telling.replaces.iter().map(|predicate| (predicate.clone(), source.name())));
            graph.sync(&telling).map(|synced| (synced, notes)).map_err(|why| why.to_string())
        });
        match outcome {
            Ok((Synced { asserted, retracted, unchanged }, notes)) => {
                report.lines.push(format!("{}: {asserted} asserted, {retracted} retracted, {unchanged} unchanged", source.name()));
                report.lines.extend(notes.into_iter().map(|note| format!("{}: {note}", source.name())));
            }
            Err(why) => {
                report.failed = true;
                report.lines.push(format!("{}: skipped: {why}", source.name()));
            }
        }
    }
    report
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "ingest" => ingest_command(rest),
        _ => north_graph::run(arguments),
    }
}

fn ingest_command(arguments: &[String]) -> u8 {
    let mut db = default_path();
    let mut sources = Vec::new();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument == "--db" {
            let Some(path) = rest.next() else { return usage("--db needs a value") };
            db = PathBuf::from(path);
        } else if let Some(source) = Source::ALL.into_iter().find(|source| source.name() == argument) {
            sources.push(source);
        } else {
            return usage(&format!("unknown argument {argument}"));
        }
    }
    if sources.is_empty() {
        sources.extend(Source::ALL);
    }
    let mut graph = match Graph::open(&db) {
        Ok(graph) => graph,
        Err(why) => {
            eprintln!("north graph ingest: {why}");
            return 1;
        }
    };
    let Some(_alone) = alone(&db) else { return 1 };
    let report = ingest(&mut graph, &Places::local(), &sources);
    for line in &report.lines {
        println!("{line}");
    }
    u8::from(report.failed)
}

fn alone(db: &std::path::Path) -> Option<std::fs::File> {
    let mut name = db.as_os_str().to_owned();
    name.push(".ingest.lock");
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&name).and_then(|file| file.lock().map(|()| file));
    lock.map_err(|why| eprintln!("north graph ingest: {}: {why}", std::path::Path::new(&name).display())).ok()
}

fn usage(why: &str) -> u8 {
    eprintln!("north graph ingest: {why}\n{USAGE}");
    2
}
