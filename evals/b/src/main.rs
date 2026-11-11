mod collect;
mod ladder;
mod proc;
mod rows;
mod scratch;
mod summary;
mod t3;
mod workload;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

const USAGE: &str = "usage:
  eval-b workload --seed S --count N --lander-host HOST --out DIR
  eval-b ladder --workload DIR --level N [--first I] [--count C] [--account A] [--hosts H,H] --work DIR --results FILE
  eval-b t3-tasks --workload DIR --count N --out FILE
  eval-b t3-rows --workload DIR --from EVAL_A_ROWS --results FILE
  eval-b summary --results FILE";

fn flags(args: &[String]) -> BTreeMap<String, String> {
    args.chunks(2).filter_map(|pair| Some((pair[0].strip_prefix("--")?.to_owned(), pair.get(1)?.clone()))).collect()
}

fn need<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags.get(key).map(String::as_str).ok_or_else(|| format!("missing --{key}\n{USAGE}"))
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_str(&fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())
}

fn append(results: &Path, rows: &[Value]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().create(true).append(true).open(results).map_err(|e| e.to_string())?;
    rows.iter().try_for_each(|row| writeln!(file, "{row}").map_err(|e| e.to_string()))
}

fn workload(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let seed: u64 = need(flags, "seed")?.parse().map_err(|_| "--seed takes a number".to_owned())?;
    let count: usize = need(flags, "count")?.parse().map_err(|_| "--count takes a number".to_owned())?;
    let out = PathBuf::from(need(flags, "out")?);
    let repo = out.join("repo");
    if repo.exists() {
        return Err(format!("{} already exists; pick a new --out", repo.display()));
    }
    let made = workload::generate(seed, count, need(flags, "lander-host")?, &repo, &out.join("expect")).map_err(|e| e.to_string())?;
    let base = scratch::commit_workload(&repo)?;
    let shared = made.tasks.iter().filter(|t| t["shared"] == true).count();
    fs::write(out.join("tasks.json"), json!({"seed": seed, "base": base, "shared_every": workload::SHARED_EVERY, "tasks": made.tasks}).to_string()).map_err(|e| e.to_string())?;
    println!("eval-b: {count} tasks, {shared} of them touch src/registry.ts, base {base}, in {}", out.display());
    Ok(())
}

fn ladder(flags: &BTreeMap<String, String>, argv: &str) -> Result<(), String> {
    let dir = PathBuf::from(need(flags, "workload")?);
    let manifest = read_json(&dir.join("tasks.json"))?;
    let level: usize = need(flags, "level")?.parse().map_err(|_| "--level takes a number".to_owned())?;
    let first: usize = flags.get("first").map_or(Ok(1), |v| v.parse()).map_err(|_| "--first takes a number".to_owned())?;
    let count: usize = flags.get("count").map_or(Ok(level), |v| v.parse()).map_err(|_| "--count takes a number".to_owned())?;
    let hosts: Vec<String> = flags.get("hosts").map_or("gex45,cpx62,cpx62b", String::as_str).split(',').map(str::to_owned).collect();
    let stamp = proc::sh("date +%m%d-%H%M%S").out.trim().to_owned();
    let work = PathBuf::from(need(flags, "work")?).join(format!("l{level}-{stamp}"));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let tasks: Vec<Value> = manifest["tasks"].as_array().into_iter().flatten().skip(first - 1).take(count).cloned().collect();
    let plan = ladder::Plan { level, tasks, account: flags.get("account").cloned().unwrap_or_else(|| "d".to_owned()), hosts, work };
    let branches: Vec<String> = plan.tasks.iter().map(|t| format!("{}-l{level}", t["id"].as_str().unwrap_or_default())).collect();
    scratch::prepare_laptop(&dir.join("repo"), &plan.hosts, &branches)?;
    std::thread::scope(|scope| {
        let handles: Vec<_> = plan.hosts.iter().map(|h| scope.spawn(|| scratch::prepare_host(h, &branches))).collect();
        handles.into_iter().try_for_each(|h| h.join().unwrap_or_else(|_| Err("a host setup panicked".to_owned())))
    })?;
    let mut lander = scratch::Lander::start(&plan.work.join("lander.log"))?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    if !lander.alive() {
        scratch::teardown(&plan.hosts);
        return Err(format!("the scratch lander did not stay up; see {}", plan.work.join("lander.log").display()));
    }
    let conditions = rows::conditions(&plan, manifest["base"].as_str().unwrap_or_default(), manifest["seed"].as_u64().unwrap_or(0));
    eprintln!("eval-b: level {level}, {} tasks, lanes ready on {:?}, lander up", plan.tasks.len(), plan.hosts);
    let runs = ladder::drive(&plan, &stamp);
    rows::verify(&plan.work)?;
    let expect = dir.join("expect");
    let command = format!("{argv} (rows in {})", plan.work.display());
    let built: Vec<Value> = runs.iter().map(|r| rows::row(&plan, r, &expect, &conditions, &command)).collect();
    if let Some(queue) = scratch::queue_dir() {
        let _ = proc::sh(&format!("cp -r '{}/logs' '{}/lander-logs'", queue.display(), plan.work.display()));
    }
    drop(lander);
    scratch::teardown(&plan.hosts);
    append(Path::new(need(flags, "results")?), &built)?;
    summary::print(&built);
    Ok(())
}

fn t3_rows(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let manifest = read_json(&PathBuf::from(need(flags, "workload")?).join("tasks.json"))?;
    let shared: serde_json::Map<String, Value> = manifest["tasks"].as_array().into_iter().flatten().map(|t| (t["id"].as_str().unwrap_or_default().to_owned(), t["shared"].clone())).collect();
    let rows = t3::rows(Path::new(need(flags, "from")?), &Value::Object(shared))?;
    append(Path::new(need(flags, "results")?), &rows)?;
    println!("eval-b: {} T3 Code rows appended", rows.len());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv = format!("eval-b {}", args.join(" "));
    let Some((verb, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let flags = flags(rest);
    let outcome = match verb.as_str() {
        "workload" => workload(&flags),
        "ladder" => ladder(&flags, &argv),
        "t3-tasks" => need(&flags, "workload").and_then(|w| t3::tasks(Path::new(w), need(&flags, "count")?.parse().map_err(|_| "--count takes a number".to_owned())?, Path::new(need(&flags, "out")?))),
        "t3-rows" => t3_rows(&flags),
        "summary" => need(&flags, "results").and_then(|r| fs::read_to_string(r).map_err(|e| e.to_string())).map(|text| summary::print(&text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect::<Vec<_>>())),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("eval-b: {error}");
            ExitCode::FAILURE
        }
    }
}
