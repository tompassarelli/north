use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::{DocumentMut, Item, Table};

use super::base::{Base, Tree};
use super::boxes::{self, Named};
use super::parts::{self, Refusal, Split};
use super::plan::Plan;
use super::{ATTEMPTS, EFFORT, MODEL, Planner, Question, Tokens, prompt};

const USAGE: &str = "usage: north work split TASKFILE --task ID --out PLAN [--root REPO]";
const STRUCTURE: &str = "structure.toml";
const SPEC_FILE: usize = 4_000;

pub type Pack<'p> = &'p dyn Fn(&Path, &[String]) -> String;

struct Options {
    taskfile: PathBuf,
    id: String,
    out: PathBuf,
    root: PathBuf,
}

fn options(arguments: &[String]) -> Result<Option<Options>, String> {
    let (mut taskfile, mut id, mut out, mut root) = (None, None, None, None);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let slot = match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--task" => &mut id,
            "--out" => &mut out,
            "--root" => &mut root,
            flag if flag.starts_with('-') => return Err(format!("unknown argument {flag}\n{USAGE}")),
            _ => {
                taskfile = Some(argument.clone());
                continue;
            }
        };
        *slot = Some(rest.next().cloned().ok_or_else(|| format!("{argument} needs a value"))?);
    }
    let (Some(taskfile), Some(id), Some(out)) = (taskfile, id, out) else { return Err(USAGE.to_owned()) };
    Ok(Some(Options { taskfile: taskfile.into(), id, out: out.into(), root: root.map_or_else(|| PathBuf::from("."), PathBuf::from) }))
}

pub fn run(arguments: &[String], planner: &Planner, pack: Pack) -> u8 {
    let outcome = options(arguments).and_then(|parsed| match parsed {
        Some(parsed) => split(&parsed, planner, pack),
        None => {
            println!("{USAGE}");
            Ok(0)
        }
    });
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north work split: {why}");
            2
        }
    }
}

struct Task {
    base: String,
    text: String,
    repository: String,
    spec: PathBuf,
}

fn field(task: &Table, defaults: Option<&Table>, key: &str) -> Option<String> {
    task.get(key).or_else(|| defaults.and_then(|defaults| defaults.get(key))).and_then(Item::as_str).map(str::to_owned)
}

fn read_task(taskfile: &Path, id: &str) -> Result<Task, String> {
    let shown = taskfile.display();
    let document: DocumentMut = std::fs::read_to_string(taskfile).map_err(|why| format!("{shown}: {why}"))?.parse().map_err(|why| format!("{shown}: {why}"))?;
    let defaults = document.get("defaults").and_then(Item::as_table);
    let task = document.get("task").and_then(Item::as_array_of_tables).into_iter().flatten().find(|task| task.get("id").and_then(Item::as_str) == Some(id)).ok_or_else(|| format!("{shown} holds no task {id}"))?;
    let need = |key: &str| field(task, defaults, key).ok_or_else(|| format!("task {id} in {shown} has no {key}"));
    let directory = taskfile.parent().unwrap_or_else(|| Path::new("."));
    let home = std::env::var("EVAL_LARGE").unwrap_or_else(|_| directory.display().to_string());
    let spec = field(task, defaults, "spec_dir").map_or_else(|| directory.join("spec"), |dir| PathBuf::from(dir.replace("$EVAL_LARGE", &home)));
    let url = need("repo_url")?;
    let repository = url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or_default().to_owned();
    Ok(Task { base: need("base")?, text: need("text")?.trim().to_owned(), repository, spec: spec.join(id) })
}

fn spec_files(directory: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_owned()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let (Ok(relative), Ok(text)) = (path.strip_prefix(directory), std::fs::read_to_string(&path)) {
                found.push((relative.display().to_string(), text.chars().take(SPEC_FILE).collect()));
            }
        }
    }
    found.sort();
    found
}

fn named_paths(text: &str, spec: &[(String, String)], tree: &Tree) -> Vec<String> {
    let words = text.split(|c: char| c.is_whitespace() || "`'\"(),;".contains(c)).map(|word| word.trim_end_matches(['.', ':']).to_owned()).filter(|word| word.contains('/'));
    let mut paths = BTreeSet::new();
    for path in words.chain(spec.iter().map(|(path, _)| path.clone())) {
        let mut candidate = path.trim_end_matches('/').to_owned();
        loop {
            if let Some(found) = [candidate.clone(), format!("{candidate}.rs"), format!("{candidate}/mod.rs")].into_iter().find(|file| tree.tracked(file)) {
                paths.insert(found);
                break;
            }
            match candidate.rsplit_once('/') {
                Some((parent, _)) if parent.contains('/') => candidate = parent.to_owned(),
                _ => break,
            }
        }
    }
    paths.into_iter().collect()
}

fn modules(root: &Path, commit: &str) -> String {
    let shown = git(root, &["show", &format!("{commit}:{STRUCTURE}")]).unwrap_or_default();
    let Ok(document) = shown.parse::<DocumentMut>() else { return String::new() };
    let strings = |module: &Table, key: &str| module.get(key).and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str()).collect::<Vec<_>>().join(" ");
    let lines: Vec<String> = document
        .get("module")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten()
        .map(|module| {
            let name = module.get("name").and_then(Item::as_str).unwrap_or_default();
            let responsibility = module.get("responsibility").and_then(Item::as_str).unwrap_or_default();
            format!("{name} [{}] may use: {}. {responsibility}", strings(module, "paths"), strings(module, "may_use"))
        })
        .collect();
    format!("\nModules of {STRUCTURE} (name [paths] and the modules it may use):\n{}\n", lines.join("\n"))
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}

pub fn overlaps(plan: &Plan, base: &dyn Base) -> Result<(), Refusal> {
    let mut holders: BTreeMap<&str, &str> = BTreeMap::new();
    let mut refused = Vec::new();
    for part in &plan.parts {
        let files: BTreeSet<&str> = part.files.iter().map(|edit| edit.path.as_str()).filter(|path| !parts::manifest(path) && !base.merged_as_set(path)).collect();
        for file in files {
            if let Some(holder) = holders.insert(file, &part.name) {
                refused.push(format!("parts {holder} and {} both name {file}; each file belongs to exactly one part", part.name));
            }
        }
    }
    if refused.is_empty() { Ok(()) } else { Err(Refusal(refused)) }
}

fn attempt(answer: &str, tree: &Tree) -> Result<Split, String> {
    let (plan, _) = Plan::parse(answer)?;
    overlaps(&plan, tree).and_then(|()| parts::split(&plan, tree)).map_err(|refusal| refusal.line())
}

fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

fn listed<'a>(files: impl IntoIterator<Item = &'a String>) -> String {
    format!("[{}]", files.into_iter().map(|file| quoted(file)).collect::<Vec<_>>().join(", "))
}

fn rendered(split: &Split, task: &str, named: &Named) -> String {
    let unbox = |line: String| line.trim_start_matches("- [ ] ").to_owned();
    if split.parts.len() < 2 {
        let files: BTreeSet<&String> = split.shared.files.iter().chain(split.parts.iter().flat_map(|part| &part.files)).collect();
        return format!("[interface]\ntext = {}\nfiles = {}\n", quoted(task), listed(files));
    }
    let mut out = format!("[interface]\ntext = {}\nfiles = {}\n", quoted(&format!("{}\n\nThe whole task, which the parts after this box finish:\n{task}", unbox(boxes::first(split, named)))), listed(&split.shared.files));
    for (index, held) in split.parts.iter().enumerate() {
        let text = format!("{}\n\nThe whole task, of which this box is one part:\n{task}", unbox(boxes::part(index + 1, split.parts.len(), held, named)));
        out.push_str(&format!("\n[[part]]\ntext = {}\nfiles = {}\n", quoted(&text), listed(&held.files)));
    }
    out
}

fn split(options: &Options, planner: &Planner, pack: Pack) -> Result<u8, String> {
    let task = read_task(&options.taskfile, &options.id)?;
    let tree = Tree::read(&options.root, &task.base)?;
    let head = git(&options.root, &["rev-parse", "HEAD"]).unwrap_or_default();
    if head.trim() != tree.commit {
        return Err(format!("{} is checked out at {}, not at the task's base {}, and the context pack reads the checkout", options.root.display(), head.trim(), tree.commit));
    }
    if !(planner.admits)(&task.repository) {
        return Err(format!("the planning call sends the task and the repository's paths to the model provider, which only a public repository may do, and {} is not one", task.repository));
    }
    let spec = spec_files(&task.spec);
    let paths = named_paths(&task.text, &spec, &tree);
    let mut described = task.text.clone();
    for (path, text) in &spec {
        described.push_str(&format!("\n\nSpec file {path} (copied over the tree before the check runs):\n{text}"));
    }
    if !paths.is_empty() {
        described.push_str(&format!("\n\nContext pack of the files the task names:\n{}", pack(&options.root, &paths)));
    }
    let mut asked = prompt::first(&described, &format!("{}{}", tree.map(), modules(&options.root, &tree.commit)));
    let mut refusal = String::new();
    for call in 1..=ATTEMPTS {
        let answer = (planner.ask)(&Question { model: MODEL, effort: EFFORT, system: prompt::SYSTEM, input: &asked })?;
        let Tokens { input, output, cache_read, cache_write } = answer.tokens;
        eprintln!("north work split: call {call} on {MODEL} at {EFFORT}: {input} input, {output} output, {cache_read} cache read, {cache_write} cache write tokens");
        match attempt(&answer.text, &tree) {
            Ok(split) => {
                eprintln!("north work split: plan of call {call}: {}", answer.text.trim());
                let id = format!("{}-{}", options.id, &tree.commit[..8]);
                std::fs::write(&options.out, rendered(&split, &task.text, &Named { id: &id, base: &tree.commit, check: "" })).map_err(|why| format!("{}: {why}", options.out.display()))?;
                println!("north work split: wrote {} with {} part(s)", options.out.display(), if split.parts.len() < 2 { 0 } else { split.parts.len() });
                return Ok(0);
            }
            Err(why) => {
                eprintln!("north work split: call {call} refused: {why}");
                asked = prompt::again(&asked, &answer.text, &why);
                refusal = why;
            }
        }
    }
    eprintln!("north work split: refused after {ATTEMPTS} calls: {refusal}");
    Ok(1)
}
