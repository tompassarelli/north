mod cargo;
mod manifest;
mod paths;
mod rewrite;
mod scan;
mod structure;
mod widen;

#[cfg(test)]
mod move_tests;
#[cfg(test)]
mod scan_tests;
#[cfg(test)]
mod workspace_tests;

use std::fs;
use std::path::{Path, PathBuf};

use crate::platform::process;

const USAGE: &str = "usage: north split move-module [--dry-run] [--from FROM] --crate CRATE [--allow-test-item NAME]... [--target-dir DIR] MODULE...";

pub struct Plan {
    pub root: PathBuf,
    pub from: String,
    pub to: String,
    pub modules: Vec<String>,
    pub dry_run: bool,
    pub allow_test_items: Vec<String>,
    pub target_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub moved: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub reexports: Vec<String>,
    pub widened: Vec<String>,
    pub test_items: Vec<String>,
    pub manifest: String,
    pub lib: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub path: String,
    pub name: String,
    pub old_dir: Option<String>,
    pub old_file: Option<String>,
    pub old_tests: Option<String>,
}

impl Module {
    fn rust_path(&self) -> String {
        self.path.replace('/', "::")
    }

    fn pending(&self) -> bool {
        self.old_dir.is_some() || self.old_file.is_some()
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let plan = match arguments.split_first() {
        Some((verb, rest)) if verb == "move-module" => parse(rest),
        _ => Err(USAGE.to_string()),
    };
    match plan.and_then(|plan| move_modules(&plan)) {
        Ok(outcome) => {
            print!("{}", report(&outcome));
            0
        }
        Err(error) => {
            eprintln!("north split move-module: {error}");
            1
        }
    }
}

fn parse(arguments: &[String]) -> Result<Plan, String> {
    let root = toplevel()?;
    let mut plan = Plan { root, from: "north-core".into(), to: String::new(), modules: Vec::new(), dry_run: false, allow_test_items: Vec::new(), target_dir: None };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let mut value = || words.next().cloned().ok_or_else(|| format!("{word} needs a value\n{USAGE}"));
        match word.as_str() {
            "--dry-run" => plan.dry_run = true,
            "--from" => plan.from = value()?,
            "--crate" => plan.to = value()?,
            "--allow-test-item" => plan.allow_test_items.push(value()?),
            "--target-dir" => plan.target_dir = Some(PathBuf::from(value()?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            module => plan.modules.push(module.trim_end_matches('/').to_string()),
        }
    }
    if plan.to.is_empty() || plan.modules.is_empty() {
        return Err(USAGE.into());
    }
    Ok(plan)
}

fn toplevel() -> Result<PathBuf, String> {
    let output = process::run("git", None, &["rev-parse", "--show-toplevel"]).map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err("not inside a git checkout".into());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

pub fn move_modules(plan: &Plan) -> Result<Outcome, String> {
    if plan.from == plan.to {
        return Err("--from and --crate name the same crate".into());
    }
    let modules = plan.modules.iter().map(|path| locate(plan, path)).collect::<Result<Vec<_>, _>>()?;
    let pending = modules.iter().filter(|module| module.pending()).cloned().collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(Outcome::default());
    }
    rewrite::refuse_back_edges(plan, &pending)?;
    let metadata = cargo::Metadata::read(plan)?;
    let generated = manifest::generate(plan, &pending, &metadata)?;
    if plan.dry_run {
        return Ok(generated.outcome(pending.iter().map(|module| module.path.clone()).collect()));
    }
    let owners = structure::owners(plan, &pending)?;
    for module in &pending {
        git_move(plan, module)?;
    }
    generated.write(plan)?;
    rewrite::workspace(plan)?;
    rewrite::callers(plan, &pending)?;
    rewrite::path_strings(plan, &pending)?;
    structure::cover(plan, &pending, &owners)?;
    let mut outcome = generated.outcome(pending.iter().map(|module| module.path.clone()).collect());
    let widened = widen::until_clean(plan)?;
    outcome.widened = widened.widened;
    outcome.test_items = widened.test_items;
    cargo::verify(plan)?;
    Ok(outcome)
}

fn locate(plan: &Plan, path: &str) -> Result<Module, String> {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let source = format!("crates/{}/src/{path}", plan.from);
    let exists = |relative: &str| plan.root.join(relative).exists();
    let old_dir = Some(source.clone()).filter(|dir| plan.root.join(dir).is_dir());
    let old_file = Some(format!("{source}.rs")).filter(|file| exists(file));
    let old_tests = Some(format!("{source}_tests.rs")).filter(|file| exists(file) && old_file.is_some());
    let module = Module { path: path.to_string(), name: name.clone(), old_dir, old_file, old_tests };
    let dest = format!("crates/{}/src/{name}", plan.to);
    if !module.pending() && !exists(&dest) && !exists(&format!("{dest}.rs")) {
        return Err(format!("no module {path} in crates/{}/src or crates/{}/src", plan.from, plan.to));
    }
    Ok(module)
}

fn git_move(plan: &Plan, module: &Module) -> Result<(), String> {
    let dest = format!("crates/{}/src/{}", plan.to, module.name);
    fs::create_dir_all(plan.root.join(format!("crates/{}/src", plan.to))).map_err(|error| error.to_string())?;
    let moves = [(&module.old_dir, dest.clone()), (&module.old_file, format!("{dest}.rs")), (&module.old_tests, format!("{dest}_tests.rs"))];
    for (from, to) in moves {
        if let Some(from) = from {
            git(&plan.root, &["mv", from, &to])?;
        }
    }
    rewrite::drop_declaration(plan, module)
}

pub fn git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = process::run("git", Some(root), arguments).map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn report(outcome: &Outcome) -> String {
    if outcome.moved.is_empty() {
        return "nothing to move: every module is already in the target crate\n".into();
    }
    let mut text = format!("moved: {}\nre-exports: {}\n", outcome.moved.join(", "), outcome.reexports.join(", "));
    for line in &outcome.widened {
        text.push_str(&format!("widened to pub: {line}\n"));
    }
    for line in &outcome.test_items {
        text.push_str(&format!("cfg(test) item now a #[doc(hidden)] pub mod, judge it: {line}\n"));
    }
    text.push_str("\ncommit-message trailers:\n");
    for dependency in &outcome.dependencies {
        text.push_str(&format!("Dependency: {} — {}\n", dependency.name, dependency.reason));
    }
    text
}
