use std::collections::HashMap;
use std::path::PathBuf;

use north_base::platform::harness::{DEFAULTS, document};
use north_graph::{Graph, Retained, Retention, Synced, Term, default_path};

use crate::places::Places;
use crate::shas::Shas;
use crate::facts::Sourced;
use crate::{corrections, coverage, crates, issues, ledger, losses, observations, pushes, ranking, reads, touches};

const USAGE: &str = "usage: north graph ingest [--db PATH] [SOURCE...]\n       north graph retain [--db PATH]\nsources: mirror ranking ledger losses pushes touches corrections coverage crates reads observations (all when none is named)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Mirror,
    Ranking,
    Ledger,
    Losses,
    Pushes,
    Touches,
    Corrections,
    Coverage,
    Crates,
    Reads,
    Observations,
}

impl Source {
    pub const ALL: [Self; 11] = [Self::Mirror, Self::Ranking, Self::Ledger, Self::Losses, Self::Pushes, Self::Touches, Self::Corrections, Self::Coverage, Self::Crates, Self::Reads, Self::Observations];

    pub fn name(self) -> &'static str {
        match self {
            Self::Mirror => "mirror",
            Self::Ranking => "ranking",
            Self::Ledger => "ledger",
            Self::Losses => "losses",
            Self::Pushes => "pushes",
            Self::Touches => "touches",
            Self::Corrections => "corrections",
            Self::Coverage => "coverage",
            Self::Crates => "crates",
            Self::Reads => "reads",
            Self::Observations => "observations",
        }
    }

    fn read(self, places: &Places, shas: &Shas) -> Result<Sourced, String> {
        match self {
            Self::Mirror => issues::read(places, shas),
            Self::Ranking => ranking::read(places),
            Self::Ledger => ledger::read(places).map(Sourced::from),
            Self::Losses => losses::read(places),
            Self::Pushes => pushes::read(places).map(Sourced::from),
            Self::Touches => touches::read(places).map(Sourced::from),
            Self::Corrections => corrections::read(places).map(Sourced::from),
            Self::Coverage => coverage::read(places).map(Sourced::from),
            Self::Crates => crates::read(places),
            Self::Reads => reads::read(places),
            Self::Observations => unreachable!("ingest appends observations through observe"),
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
    for writer in retired_writers() {
        match graph.retire(&writer) {
            Ok(0) => {}
            Ok(retracted) => report.lines.push(format!("{writer}: {retracted} retracted, its source is gone")),
            Err(why) => {
                report.failed = true;
                report.lines.push(format!("{writer}: not retired: {why}"));
            }
        }
    }
    let mut replaced: HashMap<Term, &'static str> = HashMap::new();
    for source in sources {
        if *source == Source::Observations {
            observe(graph, places, &mut report);
            continue;
        }
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
        Some((verb, rest)) if verb == "retain" => retain_command(rest),
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
    let marked = graph.mark(now()).map_err(|why| eprintln!("north graph ingest: no time mark: {why}"));
    u8::from(report.failed || marked.is_err())
}

fn retain_command(arguments: &[String]) -> u8 {
    let db = match arguments {
        [] => default_path(),
        [flag, path] if flag == "--db" => PathBuf::from(path),
        _ => return usage("retain takes only --db PATH"),
    };
    let mut graph = match Graph::open(&db) {
        Ok(graph) => graph,
        Err(why) => {
            eprintln!("north graph retain: {why}");
            return 1;
        }
    };
    let Some(_alone) = alone(&db) else { return 1 };
    let retention = Retention { retired: retired_writers(), horizon_days: north_base::platform::harness::default_number("graph_retain_days") as i64, now: now() };
    match graph.retain(&retention) {
        Ok(Retained { horizon, occurrences, withdrawals, terms, transactions, longest_ms, delete_ms, full_vacuum, vacuum_ms }) => {
            let horizon = horizon.map_or_else(|| format!("no horizon yet: no time mark is {} days old", retention.horizon_days), |horizon| format!("history kept from occurrence {horizon}"));
            let vacuum = if full_vacuum { "full vacuum, now incremental" } else { "incremental vacuum" };
            println!("retained: {occurrences} occurrences, {withdrawals} withdrawals and {terms} terms deleted in {delete_ms} ms over {transactions} write transactions, the longest {longest_ms} ms; {vacuum} {vacuum_ms} ms; {horizon}");
            0
        }
        Err(why) => {
            eprintln!("north graph retain: {why}");
            1
        }
    }
}

fn retired_writers() -> Vec<String> {
    document(DEFAULTS)["graph_retired_writers"]["value"].as_array().into_iter().flatten().filter_map(|writer| writer.as_str().map(str::to_owned)).collect()
}

fn now() -> i64 {
    north_base::foundation::clock::Timestamp::now().unix_seconds()
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

fn observe(graph: &mut Graph, places: &Places, report: &mut Report) {
    match observations::append(graph, places) {
        Ok(observations::Appended { asserted, unchanged, notes }) => {
            report.lines.push(format!("observations: {asserted} asserted, 0 retracted, {unchanged} unchanged"));
            report.lines.extend(notes.into_iter().map(|note| format!("observations: {note}")));
        }
        Err(why) => {
            report.failed = true;
            report.lines.push(format!("observations: skipped: {why}"));
        }
    }
}
