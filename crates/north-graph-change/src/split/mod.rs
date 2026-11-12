pub mod base;
pub mod boxes;
pub mod parts;
pub mod plan;
pub mod prompt;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::{Value, json};

use base::Tree;
use boxes::Named;
use plan::Plan;

const USAGE: &str = "usage: north graph split [--repo DIR] [--record FILE] [--plan FILE | --prompt FILE] [--model MODEL] [--effort EFFORT] --base SHA (--task FILE | -)
       north graph split --check ID --parts N [--repo DIR] [--record FILE]";
const MODEL: &str = "claude-opus-5-5";
const EFFORT: &str = "medium";
const ATTEMPTS: usize = 2;
const FLAGS: [&str; 10] = ["--prompt", "--repo", "--record", "--plan", "--model", "--effort", "--base", "--task", "--check", "--parts"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

pub struct Answer {
    pub text: String,
    pub tokens: Tokens,
}

#[derive(Clone, Copy)]
pub struct Question<'q> {
    pub model: &'q str,
    pub effort: &'q str,
    pub system: &'q str,
    pub input: &'q str,
}

pub struct Planner<'p> {
    pub admits: &'p dyn Fn(&str) -> bool,
    pub ask: &'p dyn Fn(&Question) -> Result<Answer, String>,
}

struct Options {
    repo: Option<PathBuf>,
    record: Option<PathBuf>,
    plan: Option<PathBuf>,
    prompt: Option<PathBuf>,
    model: Option<String>,
    effort: Option<String>,
    base: Option<String>,
    task: Option<String>,
    check: Option<String>,
    parts: Option<usize>,
}

fn options(arguments: &[String]) -> Result<Options, String> {
    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument == "-" {
            values.insert("--task", argument.clone());
            continue;
        }
        let flag = FLAGS.iter().find(|flag| **flag == argument).ok_or_else(|| format!("unknown argument {argument}\n{USAGE}"))?;
        values.insert(flag, rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"))?);
    }
    let mut take = |flag: &str| values.remove(flag);
    Ok(Options {
        repo: take("--repo").map(PathBuf::from),
        record: take("--record").map(PathBuf::from),
        plan: take("--plan").map(PathBuf::from),
        prompt: take("--prompt").map(PathBuf::from),
        model: take("--model"),
        effort: take("--effort"),
        base: take("--base"),
        task: take("--task"),
        check: take("--check"),
        parts: take("--parts").map(|count| count.parse().map_err(|_| "--parts takes a number".to_owned())).transpose()?,
    })
}

pub fn run(arguments: &[String], planner: &Planner) -> u8 {
    let outcome = options(arguments).and_then(|parsed| match (&parsed.check, parsed.parts) {
        (Some(id), Some(parts)) => check(&parsed, id, parts),
        (Some(_), None) => Err(format!("--check needs --parts\n{USAGE}")),
        _ => split(&parsed, planner),
    });
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north graph split: {why}");
            2
        }
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}

fn public(root: &Path, planner: &Planner) -> Result<(), String> {
    let origin = git(root, &["remote", "get-url", "origin"]).ok_or("the repository has no origin, so its visibility is unknown")?;
    let repository = origin.trim().trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or_default().to_owned();
    if (planner.admits)(&repository) {
        Ok(())
    } else {
        Err(format!("the planning call sends the task and the repository's paths to the model provider, which only a public repository may do, and {repository} is not one"))
    }
}

fn id(task: &str, commit: &str) -> String {
    let hash = task.bytes().chain(commit.bytes()).fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("{:08x}", hash >> 32)
}

fn read_task(source: &str) -> Result<String, String> {
    if source == "-" {
        return std::io::read_to_string(std::io::stdin()).map_err(|why| format!("stdin: {why}"));
    }
    std::fs::read_to_string(source).map_err(|why| format!("{source}: {why}"))
}

fn default_record(id: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(format!(".local/state/north/splits/{id}.json"))
}

fn write(path: &Path, record: &Value) -> Result<(), String> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|why| format!("{}: {why}", directory.display()))?;
    }
    std::fs::write(path, format!("{record:#}\n")).map_err(|why| format!("{}: {why}", path.display()))
}

struct Attempts {
    calls: Vec<Value>,
    refusals: Vec<String>,
    plan: Value,
}

fn attempt(answer: &str, tree: &Tree, attempts: &mut Attempts) -> Option<parts::Split> {
    let outcome = Plan::parse(answer).map_err(|why| parts::Refusal(vec![why])).and_then(|(plan, value)| {
        attempts.plan = value;
        parts::split(&plan, tree)
    });
    outcome.map_err(|refusal| attempts.refusals.push(refusal.line())).ok()
}

fn split(options: &Options, planner: &Planner) -> Result<u8, String> {
    let started = Instant::now();
    let (Some(base), Some(source)) = (&options.base, &options.task) else { return Err(USAGE.to_owned()) };
    let root = options.repo.clone().unwrap_or_else(|| PathBuf::from("."));
    let task = read_task(source)?;
    let tree = Tree::read(&root, base)?;
    let id = id(&task, &tree.commit);
    if let Some(file) = &options.prompt {
        std::fs::write(file, prompt::first(&task, &tree.map())).map_err(|why| format!("{}: {why}", file.display()))?;
        return Ok(0);
    }
    let (model, effort) = (options.model.as_deref().unwrap_or(MODEL), options.effort.as_deref().unwrap_or(EFFORT));
    let mut attempts = Attempts { calls: Vec::new(), refusals: Vec::new(), plan: Value::Null };
    let found = match &options.plan {
        Some(file) => attempt(&std::fs::read_to_string(file).map_err(|why| format!("{}: {why}", file.display()))?, &tree, &mut attempts),
        None => {
            public(&root, planner)?;
            let mut asked = prompt::first(&task, &tree.map());
            let mut found = None;
            for _ in 0..ATTEMPTS {
                let called = Instant::now();
                let answer = (planner.ask)(&Question { model, effort, system: prompt::SYSTEM, input: &asked })?;
                let Tokens { input, output, cache_read, cache_write } = answer.tokens;
                attempts.calls.push(json!({ "ms": called.elapsed().as_millis(), "input_tokens": input, "output_tokens": output, "cache_read_tokens": cache_read, "cache_write_tokens": cache_write }));
                found = attempt(&answer.text, &tree, &mut attempts);
                match (&found, attempts.refusals.last()) {
                    (None, Some(refusal)) => asked = prompt::again(&asked, &answer.text, refusal),
                    _ => break,
                }
            }
            found
        }
    };
    let record_path = options.record.clone().unwrap_or_else(|| default_record(&id));
    let parts = found.as_ref().map_or(0, |found| found.parts.len());
    let check = format!("north graph split --check {id} --parts {parts} --record {}", record_path.display());
    let record = json!({
        "id": id, "base": tree.commit, "model": model, "effort": effort, "planned_by": if options.plan.is_some() { "file" } else { "model" },
        "took_ms": started.elapsed().as_millis(), "calls": attempts.calls, "refusals": attempts.refusals, "plan": attempts.plan,
        "split": found.as_ref().map(boxes::recorded), "check": check, "first_commit": Value::Null,
    });
    write(&record_path, &record)?;
    match found {
        Some(found) => {
            println!("{}", boxes::markdown(&found, &Named { id: &id, base: &tree.commit, check: &check }));
            Ok(0)
        }
        None => {
            eprintln!("north graph split: refused: {}", attempts.refusals.last().map_or("", String::as_str));
            eprintln!("north graph split: recorded in {}", record_path.display());
            Ok(1)
        }
    }
}

fn check(options: &Options, id: &str, parts: usize) -> Result<u8, String> {
    let root = options.repo.clone().unwrap_or_else(|| PathBuf::from("."));
    git(&root, &["fetch", "-q", "origin", "main"]).ok_or("cannot fetch origin/main")?;
    let landed = |which: &str| git(&root, &["log", "-1", "--format=%H", "-E", &format!("--grep=^{}$", boxes::trailer(id, which)), "origin/main"]).map(|found| found.trim().to_owned()).filter(|found| !found.is_empty());
    let first = landed("first");
    let missing: Vec<String> = (1..=parts).map(|index| format!("part {index}")).filter(|which| landed(which).is_none()).collect();
    let stubs = git(&root, &["grep", "-l", "-F", &boxes::stub(id), "origin/main", "--"]).unwrap_or_default();
    let stubbed: Vec<&str> = stubs.lines().filter_map(|line| line.strip_prefix("origin/main:")).collect();
    if let (Some(path), Some(commit)) = (&options.record, &first) {
        let mut record: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()))?).map_err(|why| format!("{}: {why}", path.display()))?;
        record["first_commit"] = json!(commit);
        write(path, &record)?;
    }
    println!("first box: {}", first.as_deref().unwrap_or("not on origin/main"));
    println!("parts not on origin/main: {}", if missing.is_empty() { "none".to_owned() } else { missing.join(", ") });
    println!("files on origin/main holding a {} stub: {}", boxes::stub(id), if stubbed.is_empty() { "none".to_owned() } else { stubbed.join(" ") });
    Ok(u8::from(first.is_none() || !missing.is_empty() || !stubbed.is_empty()))
}
