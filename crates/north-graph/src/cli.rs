use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::json;

use crate::json::term_to_json;
use crate::query::{Answer, parse_query};
use crate::store::{Action, ActionKind, Graph, Receipt, Report, default_path};
use crate::term::{Term, Triple};
use crate::text::parse_arg;

const USAGE: &str = "usage: north graph tell|retract [--db PATH] [--writer NAME] (T1 T2 T3 | TRIPLE)
       north graph show [--db PATH] [--json] T1
       north graph ask [--db PATH] [--json] (QUERY_JSON | --file PATH | -)
       north graph validate [--db PATH]
terms: \"text\" 42 1.5 true :keyword @SECONDS.NANOS9 (T1 T2 T3); a bare word is a string";

struct Options {
    db: PathBuf,
    writer: String,
    json: bool,
    file: Option<PathBuf>,
    positional: Vec<String>,
}

fn options(arguments: &[String]) -> Result<Options, String> {
    let mut parsed = Options {
        db: default_path(),
        writer: std::env::var("NORTH_GRAPH_WRITER").ok().filter(|writer| !writer.is_empty()).unwrap_or_else(|| "cli".to_string()),
        json: false,
        file: None,
        positional: Vec::new(),
    };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--db" => parsed.db = PathBuf::from(value("--db")?),
            "--writer" => parsed.writer = value("--writer")?,
            "--file" => parsed.file = Some(PathBuf::from(value("--file")?)),
            "--json" => parsed.json = true,
            flag if flag.len() > 1 && flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => parsed.positional.push(argument.clone()),
        }
    }
    Ok(parsed)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((verb, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let outcome = options(rest).and_then(|options| dispatch(verb, &options));
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north graph {verb}: {why}");
            if why.starts_with("usage") { 2 } else { 1 }
        }
    }
}

fn dispatch(verb: &str, options: &Options) -> Result<u8, String> {
    match verb {
        "tell" => write(ActionKind::Assert, options),
        "retract" => write(ActionKind::Retract, options),
        "show" => show(options),
        "ask" => ask(options),
        "validate" => validate(options),
        _ => Err(format!("usage: unknown verb {verb}\n{USAGE}")),
    }
}

fn open(options: &Options) -> Result<Graph, String> {
    Graph::open(&options.db).map_err(|why| why.to_string())
}

fn triple_of(positional: &[String]) -> Result<Triple, String> {
    let terms = positional.iter().map(|text| parse_arg(text)).collect::<Result<Vec<_>, _>>()?;
    match <[Term; 3]>::try_from(terms) {
        Ok([t1, t2, t3]) => Ok(Triple::new(t1, t2, t3)),
        Err(mut terms) if terms.len() == 1 => match terms.remove(0) {
            Term::Triple(triple) => Ok(*triple),
            other => Err(format!("usage: {other} is not a triple\n{USAGE}")),
        },
        Err(terms) => Err(format!("usage: expected T1 T2 T3 or one triple, got {} terms\n{USAGE}", terms.len())),
    }
}

fn write(kind: ActionKind, options: &Options) -> Result<u8, String> {
    let triple = triple_of(&options.positional)?;
    let mut graph = open(options)?;
    let action = Action { kind, triple };
    let receipt = graph.apply(&options.writer, &[action]).map_err(|why| why.to_string())?.remove(0);
    println!("{}", describe(&receipt));
    Ok(0)
}

fn describe(receipt: &Receipt) -> String {
    match (receipt.kind, &receipt.withdrew) {
        (ActionKind::Assert, _) => format!("asserted {}", receipt.coordinate),
        (ActionKind::Retract, Some(assertion)) => format!("retracted {} withdrawing {assertion}", receipt.coordinate),
        (ActionKind::Retract, None) => format!("retracted {} with no live match (stateChanged false)", receipt.coordinate),
    }
}

fn show(options: &Options) -> Result<u8, String> {
    let [subject] = options.positional.as_slice() else {
        return Err(format!("usage: show takes one T1\n{USAGE}"));
    };
    let live = open(options)?.show(&parse_arg(subject)?).map_err(|why| why.to_string())?;
    for entry in live {
        if options.json {
            let triple = Term::from(entry.triple);
            println!("{}", json!({"triple": term_to_json(&triple), "occurrences": entry.occurrences}));
        } else {
            println!("{}\t{}", entry.triple, entry.occurrences);
        }
    }
    Ok(0)
}

fn query_text(options: &Options) -> Result<String, String> {
    if let Some(path) = &options.file {
        return std::fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()));
    }
    match options.positional.as_slice() {
        [text] if text == "-" => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|why| why.to_string())?;
            Ok(text)
        }
        [text] => Ok(text.clone()),
        _ => Err(format!("usage: ask takes one query\n{USAGE}")),
    }
}

fn ask(options: &Options) -> Result<u8, String> {
    let query = parse_query(&query_text(options)?).map_err(|why| why.to_string())?;
    let answer = open(options)?.ask(&query).map_err(|why| why.to_string())?;
    let mut out = std::io::stdout().lock();
    print_answer(&mut out, &answer, options.json).map_err(|why| why.to_string())?;
    Ok(0)
}

fn print_answer(out: &mut impl Write, answer: &Answer, as_json: bool) -> std::io::Result<()> {
    if as_json {
        let rows: Vec<_> = answer.rows.iter().map(|row| row.iter().map(term_to_json).collect::<Vec<_>>()).collect();
        return writeln!(out, "{}", json!({"columns": answer.columns, "rows": rows, "steps": answer.steps}));
    }
    for row in &answer.rows {
        let cells: Vec<String> = row.iter().map(ToString::to_string).collect();
        writeln!(out, "{}", cells.join("\t"))?;
    }
    Ok(())
}

fn validate(options: &Options) -> Result<u8, String> {
    let report = open(options)?.validate().map_err(|why| why.to_string())?;
    println!("{}", summary(&report));
    for problem in &report.problems {
        println!("problem: {problem}");
    }
    Ok(u8::from(!report.is_sound()))
}

fn summary(report: &Report) -> String {
    let verdict = if report.is_sound() { "sound" } else { "unsound" };
    format!(
        "{verdict}: {} terms, {} occurrences, {} live occurrences, {} live triples",
        report.terms, report.occurrences, report.live_occurrences, report.live_triples
    )
}
