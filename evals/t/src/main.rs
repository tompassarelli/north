mod classify;
mod context;
mod pin;
mod proc;
mod row;
mod transfer;
mod world;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

const USAGE: &str = "usage:
  eval-t fetch --into DIR [--pin COMMIT]
  eval-t classify --checkout DIR [--pin COMMIT] [--out FILE]
  eval-t classify --checkout DIR --check FILE
  eval-t context --transcript FILE [--bare FILE]
  eval-t transfer-fixture --state DIR
  eval-t world --checkout DIR --case ID --into DIR
  eval-t grade --checkout DIR --grader DIR --node PROGRAM --case ID --arm NAME --transcript FILE --cwd DIR --out DIR [--stamp JSON]";

fn flags(args: &[String]) -> BTreeMap<String, String> {
    args.chunks(2).filter_map(|pair| Some((pair[0].strip_prefix("--")?.to_owned(), pair.get(1)?.clone()))).collect()
}

fn need<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags.get(key).map(String::as_str).ok_or_else(|| format!("missing --{key}\n{USAGE}"))
}

fn pretty(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map(|text| text + "\n").map_err(|e| e.to_string())
}

fn classify(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let checkout = PathBuf::from(need(flags, "checkout")?);
    if let Some(frozen) = flags.get("check") {
        let text = fs::read_to_string(frozen).map_err(|e| format!("{frozen}: {e}"))?;
        let list: Value = serde_json::from_str(&text).map_err(|e| format!("{frozen}: {e}"))?;
        let pin = list["pin"].as_str().ok_or_else(|| format!("{frozen} names no pin"))?;
        if pretty(&classify::classify(&checkout, pin)?)? != text {
            return Err(format!("{frozen} is not what the rule gives at {pin}; the frozen list and the checkout disagree"));
        }
        println!("eval-t: {frozen} is the rule's list at {pin}: {}", list["counts"]);
        return Ok(());
    }
    let pin = flags.get("pin").map_or(pin::PIN, String::as_str);
    let list = classify::classify(&checkout, pin)?;
    match flags.get("out") {
        Some(out) => fs::write(Path::new(out), pretty(&list)?).map_err(|e| format!("{out}: {e}"))?,
        None => print!("{}", pretty(&list)?),
    }
    eprintln!("eval-t: at {pin}: {}", list["counts"]);
    Ok(())
}

fn context(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let read = |path: &str| fs::read_to_string(path).map_err(|e| format!("{path}: {e}")).and_then(|text| context::first_request(&text));
    let mut row = read(need(flags, "transcript")?)?;
    if let Some(bare) = flags.get("bare") {
        row["added_over_bare"] = context::over_bare(&row, &read(bare)?);
    }
    println!("{row}");
    Ok(())
}

fn grade(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let grader = Path::new(need(flags, "grader")?);
    pin::graders_at(grader, pin::PIN)?;
    let case = world::case(Path::new(need(flags, "checkout")?), need(flags, "case")?)?;
    let transcript = need(flags, "transcript")?;
    let text = fs::read_to_string(transcript).map_err(|e| format!("{transcript}: {e}"))?;
    let row = row::row(&case, &text, need(flags, "cwd")?);
    let out = fs::canonicalize(need(flags, "out")?).map_err(|e| format!("--out: {e}"))?;
    let rows = out.join("row.jsonl");
    fs::write(&rows, format!("{row}\n")).map_err(|e| format!("{}: {e}", rows.display()))?;
    let regrade = proc::run_in(&grader.join("evals"), need(flags, "node")?, &["mcp-tool-use/regrade.ts", &rows.to_string_lossy()]);
    let table = proc::must(regrade, "T3 Code's regrade.ts")?;
    fs::write(out.join("regrade.txt"), &table).map_err(|e| e.to_string())?;
    let mut stamp: Value = flags.get("stamp").map_or(Ok(json!({})), |stamp| serde_json::from_str(stamp)).map_err(|e| format!("--stamp: {e}"))?;
    stamp["arm"] = json!(need(flags, "arm")?);
    stamp["first_request"] = context::first_request(&text).unwrap_or(Value::Null);
    stamp["grader"] = json!({"release": pin::RELEASE, "pin": pin::PIN, "script": pin::GRADERS[1], "run_ts_sha256": pin::hashes(grader, &[pin::GRADERS[0].to_owned()])?[pin::GRADERS[0]]["sha256"]});
    println!("{}", row::graded(&table, &row, stamp)?);
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    let flags = flags(rest);
    match command.as_str() {
        "fetch" => pin::fetch(flags.get("pin").map_or(pin::PIN, String::as_str), Path::new(need(&flags, "into")?)),
        "classify" => classify(&flags),
        "context" => context(&flags),
        "transfer-fixture" => transfer::fixture(Path::new(need(&flags, "state")?)).map(|manifest| println!("{manifest:#}")),
        "world" => world::build(Path::new(need(&flags, "checkout")?), need(&flags, "case")?, Path::new(need(&flags, "into")?)).map(|manifest| println!("{manifest:#}")),
        "grade" => grade(&flags),
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("eval-t: {message}");
            ExitCode::FAILURE
        }
    }
}
