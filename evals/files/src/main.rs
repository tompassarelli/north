mod cap;
mod classify;
mod map;
mod pickers;
mod proc;
mod report;
mod task;
mod variants;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use serde_json::Value;

use task::{Task, Tree};
use variants::Ranked;

const USAGE: &str = "usage: eval-files --tasks TASKS.json --work DIR [--wisp DIR] [--model M] [--accounts a,d,e]
  Scores five file pickers on the recorded tasks: the graph, text search, a fast-model classifier on the brief and a repository map,
  the classifier on the brief and the top search and graph candidates, and unions of them. Raw classifier replies are kept in DIR/raw
  and reused on a rerun; delete DIR/raw to ask again. The summary goes to DIR/summary.md and to stdout.";
const CANDIDATE_GREP: usize = 30;
const CANDIDATE_GRAPH: usize = 15;

fn flags(args: &[String]) -> BTreeMap<String, String> {
    args.chunks(2).filter_map(|pair| Some((pair[0].strip_prefix("--")?.to_owned(), pair.get(1)?.clone()))).collect()
}

struct Prepared {
    tree: Tree,
    truth: BTreeSet<String>,
    graph: Vec<String>,
    grep: Vec<String>,
    map: String,
    candidates: String,
    candidate_files: Vec<String>,
}

fn cached(raw: &Path, name: &str, make: impl FnOnce() -> Result<String, String>) -> Result<String, String> {
    let file = raw.join(name);
    if let Ok(held) = fs::read_to_string(&file) {
        return Ok(held);
    }
    let made = make()?;
    fs::write(&file, &made).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(made)
}

fn prepare(task: &Task, wisp: &Path, raw: &Path) -> Result<Prepared, String> {
    let tree = Tree::at(&task.repo, &task.base)?;
    let truth = task::truth(task, wisp)?;
    let graph_text = cached(raw, &format!("{}.graph.txt", task.id), || pickers::graph_output(task))?;
    let graph = pickers::graph_rank(&graph_text, &tree);
    let grep = pickers::grep_rank(task)?;
    let symbols = map::symbols_by_path(&task.repo, &task.base, &tree)?;
    let none = Vec::new();
    let candidate_files = cap::union_in_order(&[&grep[..grep.len().min(CANDIDATE_GREP)], &graph[..graph.len().min(CANDIDATE_GRAPH)]]);
    let candidates = candidate_files.iter().map(|path| map::entry(path, symbols.get(path).unwrap_or(&none), 6)).collect::<Vec<_>>().join("\n");
    Ok(Prepared { map: map::repository_map(&tree, &symbols), tree, truth, graph, grep, candidates, candidate_files })
}

struct Job {
    task: usize,
    kind: &'static str,
    input: String,
}

fn run_jobs(jobs: Vec<Job>, accounts: &[classify::Account], model: &str, raw: &Path, ids: &[String]) -> Result<(), String> {
    let queue = Mutex::new(jobs.into_iter().enumerate().collect::<Vec<_>>());
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for worker in 0..accounts.len() {
            let (queue, failures) = (&queue, &failures);
            scope.spawn(move || {
                while let Some((number, job)) = queue.lock().unwrap().pop() {
                    let file = raw.join(format!("{}.{}.json", ids[job.task], job.kind));
                    if file.exists() {
                        continue;
                    }
                    match classify::ask(accounts, worker + number, model, &job.input) {
                        Ok(reply) => {
                            let _ = fs::write(&file, reply.to_string());
                            eprintln!("{} {}: {} ms, {} tokens in", ids[job.task], job.kind, reply["wall_ms"], reply["input_tokens"]);
                        }
                        Err(why) => failures.lock().unwrap().push(format!("{} {}: {why}", ids[job.task], job.kind)),
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    if failures.is_empty() { Ok(()) } else { Err(failures.join("\n")) }
}

fn proposal(raw: &Path, id: &str, kind: &str, tree: &Tree) -> Result<(Vec<String>, Value, usize), String> {
    let file = raw.join(format!("{id}.{kind}.json"));
    let reply: Value = serde_json::from_str(&fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?).map_err(|e| e.to_string())?;
    let parsed = classify::parse(reply["answer"].as_str().unwrap_or(""), tree);
    Ok((parsed.files, reply, parsed.invented))
}

fn ranked_of(name: &str, files: Vec<String>, visible: BTreeSet<String>) -> Ranked {
    Ranked { name: name.to_owned(), files, visible, classifier: None, invented: 0 }
}

fn assemble(task: &Task, data: &Prepared, raw: &Path) -> Result<(Vec<Ranked>, Vec<Ranked>), String> {
    let mapped: BTreeSet<String> = data.tree.sizes.keys().filter(|path| map::mapped(path)).cloned().collect();
    let mut sources = vec![
        ranked_of(variants::GRAPH, data.graph.clone(), data.graph.iter().cloned().collect()),
        ranked_of(variants::GREP, data.grep.clone(), data.grep.iter().cloned().collect()),
    ];
    for (name, kind, visible) in [(variants::MAP, "map", mapped), (variants::CANDIDATES, "candidates", data.candidate_files.iter().cloned().collect())] {
        let (files, reply, invented) = proposal(raw, &task.id, kind, &data.tree)?;
        sources.push(Ranked { classifier: Some(reply), invented, ..ranked_of(name, files, visible) });
    }
    let unions = variants::unions(&sources);
    Ok((sources, unions))
}

fn run(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let need = |key: &str| flags.get(key).ok_or_else(|| format!("missing --{key}\n{USAGE}"));
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let work = PathBuf::from(need("work")?);
    let raw = work.join("raw");
    fs::create_dir_all(&raw).map_err(|e| e.to_string())?;
    let wisp = flags.get("wisp").map_or_else(|| PathBuf::from(&home).join("code/wisp/main"), PathBuf::from);
    let model = flags.get("model").map_or("claude-haiku-5-5", String::as_str);
    let accounts = classify::accounts(flags.get("accounts").map_or("a,d,e", String::as_str), &home)?;
    let tasks = task::load(Path::new(need("tasks")?))?;
    let prepared: Vec<Prepared> = tasks.iter().map(|task| prepare(task, &wisp, &raw)).collect::<Result<_, _>>()?;
    let mut jobs = Vec::new();
    for (index, (task, data)) in tasks.iter().zip(&prepared).enumerate() {
        jobs.push(Job { task: index, kind: "map", input: classify::whole_map_input(&task.text, &data.map) });
        jobs.push(Job { task: index, kind: "candidates", input: classify::candidates_input(&task.text, &data.candidates) });
    }
    let ids: Vec<String> = tasks.iter().map(|task| task.id.clone()).collect();
    run_jobs(jobs, &accounts, model, &raw, &ids)?;
    let mut results = Vec::new();
    for (task, data) in tasks.iter().zip(&prepared) {
        let (sources, unions) = assemble(task, data, &raw)?;
        results.push(report::TaskResult { id: task.id.clone(), truth: data.truth.clone(), tree: &data.tree, sources, unions, map_bytes: data.map.len() });
    }
    let summary = report::summary(&results, model);
    fs::write(work.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    print!("{summary}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&flags(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("eval-files: {why}");
            ExitCode::FAILURE
        }
    }
}
