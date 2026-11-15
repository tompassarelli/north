use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use north_base::platform::harness::{DEFAULTS, document};
use north_graph::{Atom, Graph, Retained, Retention, Synced, Telling, Term, default_path};

use crate::places::Places;
use crate::shas::Shas;
use crate::observations;
use crate::source::Source;
use crate::crates;
use crate::facts::{Facts, Sourced, text};

const USAGE: &str = "usage: north graph ingest [--db PATH] [SOURCE... | crates@COMMIT...]\n       north graph retain [--db PATH]\nsources: mirror ranking ledger losses pushes touches corrections coverage crates reads observations (all when none is named); crates@COMMIT adds one commit's crates beside the head's";

const RETAIN_PATIENCE: Duration = Duration::from_secs(10 * 60);
const WRITER: &str = "graph-ingest";
const SUBJECT: &str = "graph-ingest";
const REVISIONS: [&str; 3] = ["reads-commit", "crates-commit", "commit"];


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
    let mut outcomes: Vec<(&'static str, String)> = Vec::new();
    for source in sources {
        if *source == Source::Observations {
            outcomes.push((source.name(), observe(graph, places, &mut report)));
            continue;
        }
        let outcome = source.read(places, &shas).and_then(|Sourced { telling, notes }| {
            if let Some((predicate, other)) = telling.replaces.iter().find_map(|predicate| replaced.get(predicate).map(|other| (predicate, other))) {
                return Err(format!("it replaces {predicate}, which {other} already replaces in this run"));
            }
            replaced.extend(telling.replaces.iter().map(|predicate| (predicate.clone(), source.name())));
            let revision = revision(&telling);
            let facts = telling.facts.len();
            graph.sync(&telling).map(|synced| (synced, notes, revision, facts)).map_err(|why| why.to_string())
        });
        match outcome {
            Ok((Synced { asserted, retracted, unchanged }, notes, revision, facts)) => {
                report.lines.push(format!("{}: {asserted} asserted, {retracted} retracted, {unchanged} unchanged", source.name()));
                report.lines.extend(notes.into_iter().map(|note| format!("{}: {note}", source.name())));
                let revision = revision.map_or_else(String::new, |(predicate, object)| format!(", {predicate} {object}"));
                outcomes.push((source.name(), format!("{facts} facts{revision}")));
            }
            Err(why) => {
                report.failed = true;
                report.lines.push(format!("{}: skipped: {why}", source.name()));
                outcomes.push((source.name(), format!("refused: {why}")));
            }
        }
    }
    if let Err(why) = record(graph, &outcomes) {
        report.failed = true;
        report.lines.push(format!("ingest outcome not recorded: {why}"));
    }
    report
}

fn ingest_at(graph: &mut Graph, places: &Places, revision: &str, report: &mut Report) {
    let outcome = crates::read_at(places, revision).and_then(|telling| graph.sync(&telling).map_err(|why| why.to_string()));
    match outcome {
        Ok(Synced { asserted, retracted, unchanged }) => report.lines.push(format!("crates@{revision}: {asserted} asserted, {retracted} retracted, {unchanged} unchanged")),
        Err(why) => {
            report.failed = true;
            report.lines.push(format!("crates@{revision}: skipped: {why}"));
        }
    }
}

/// The commit a source's facts answer from: the first fact naming one of the revision predicates a source tells.
fn revision(telling: &Telling) -> Option<(String, String)> {
    telling.facts.iter().find_map(|told| match (&told.triple.t2, &told.triple.t3) {
        (Term::Atom(Atom::Keyword(predicate)), Term::Atom(Atom::String(object))) if REVISIONS.contains(&predicate.as_str()) => Some((predicate.clone(), object.clone())),
        _ => None,
    })
}

/// Tells each source's last outcome under the `graph-ingest` subject, so `north graph show graph-ingest` says what a run of each source did; a rerun that changes nothing adds nothing.
fn record(graph: &mut Graph, outcomes: &[(&'static str, String)]) -> Result<(), String> {
    if outcomes.is_empty() {
        return Ok(());
    }
    let predicates: Vec<String> = outcomes.iter().map(|(source, _)| format!("ingest-{source}")).collect();
    let names: Vec<&str> = predicates.iter().map(String::as_str).collect();
    let mut facts = Facts::new(&names);
    for ((_, outcome), predicate) in outcomes.iter().zip(&names) {
        facts.tell(WRITER, text(SUBJECT), predicate, text(outcome.as_str()));
    }
    graph.sync(&facts.finish()).map(|_| ()).map_err(|why| why.to_string())
}

pub fn run(arguments: &[String]) -> u8 {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok();
    match arguments.split_first() {
        Some((verb, rest)) if verb == "ingest" => ingest_command(rest, cgroup.as_deref()),
        Some((verb, rest)) if verb == "retain" => retain_command(rest),
        _ => north_graph::run_guarded(arguments, &|verb, writer| crate::trusted_writers::refusal(verb, writer, cgroup.as_deref())),
    }
}

struct IngestArguments {
    db: PathBuf,
    sources: Vec<Source>,
    commits: Vec<String>,
}

fn ingest_arguments(arguments: &[String]) -> Result<IngestArguments, String> {
    let mut parsed = IngestArguments { db: default_path(), sources: Vec::new(), commits: Vec::new() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument == "--db" {
            let path = rest.next().ok_or("--db needs a value")?;
            parsed.db = PathBuf::from(path);
        } else if let Some(source) = Source::ALL.into_iter().find(|source| source.name() == argument) {
            parsed.sources.push(source);
        } else if let Some(revision) = argument.strip_prefix("crates@") {
            parsed.commits.push(revision.to_owned());
        } else {
            return Err(format!("unknown argument {argument}"));
        }
    }
    if parsed.sources.is_empty() && parsed.commits.is_empty() {
        parsed.sources.extend(Source::ALL);
    }
    Ok(parsed)
}

fn refusals(parsed: &IngestArguments, cgroup: Option<&str>) -> Vec<String> {
    let mut refused: Vec<String> = parsed.sources.iter().filter_map(|source| crate::trusted_writers::ingest_refusal(source.name(), source.writers(), cgroup)).collect();
    if !parsed.commits.is_empty() {
        refused.extend(crate::trusted_writers::ingest_refusal(Source::Crates.name(), Source::Crates.writers(), cgroup));
    }
    refused
}

fn ingest_command(arguments: &[String], cgroup: Option<&str>) -> u8 {
    let parsed = match ingest_arguments(arguments) {
        Ok(parsed) => parsed,
        Err(why) => return usage(&why),
    };
    let refused = refusals(&parsed, cgroup);
    if !refused.is_empty() {
        for why in &refused {
            eprintln!("north graph ingest: {why}");
        }
        return 1;
    }
    let IngestArguments { db, sources, commits } = parsed;
    let mut graph = match Graph::open(&db) {
        Ok(graph) => graph,
        Err(why) => {
            eprintln!("north graph ingest: {why}");
            return 1;
        }
    };
    let Some(_alone) = alone(&db) else { return 1 };
    let places = Places::local();
    let mut report = ingest(&mut graph, &places, &sources);
    for revision in &commits {
        ingest_at(&mut graph, &places, revision, &mut report);
    }
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
    let Some(_alone) = alone(&db) else { return 1 };
    let mut graph = match Graph::open_patiently(&db, RETAIN_PATIENCE) {
        Ok(graph) => graph,
        Err(why) => {
            eprintln!("north graph retain: {why}");
            return 1;
        }
    };
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
    north_foundation::foundation::clock::Timestamp::now().unix_seconds()
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

fn observe(graph: &mut Graph, places: &Places, report: &mut Report) -> String {
    match observations::append(graph, places) {
        Ok(observations::Appended { asserted, unchanged, notes }) => {
            report.lines.push(format!("observations: {asserted} asserted, 0 retracted, {unchanged} unchanged"));
            report.lines.extend(notes.into_iter().map(|note| format!("observations: {note}")));
            format!("{} facts", asserted + unchanged)
        }
        Err(why) => {
            report.failed = true;
            report.lines.push(format!("observations: skipped: {why}"));
            format!("refused: {why}")
        }
    }
}
