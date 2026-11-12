use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::proc::git;
use crate::task::Tree;

const SYMBOLS_ASKED: usize = 40;
const COMMITS_READ: usize = 60;
const WIDE_COMMIT: usize = 25;
const DEFINITION: &str = r"^\s*(pub(\([a-z]+\))? )?(async )?(unsafe )?(fn|struct|enum|trait|type|const|static|mod|macro_rules!) ";

pub struct Reach {
    tests_of: BTreeMap<String, BTreeSet<String>>,
}

pub struct Grounded {
    pub named: Vec<String>,
    pub defining: Vec<String>,
    pub cochanged: Vec<String>,
    pub tests: Vec<String>,
}

impl Grounded {
    pub fn ranked(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        [&self.named, &self.defining, &self.cochanged, &self.tests].into_iter().flatten().filter(|path| seen.insert((*path).clone())).cloned().collect()
    }
}

fn module_file(roots: &BTreeMap<String, String>, test: &str, tree: &Tree) -> Option<String> {
    let segments: Vec<&str> = test.strip_prefix("test:")?.split("::").collect();
    let root = roots.get(*segments.first()?)?;
    let prefix = if root.is_empty() || root == "." { String::new() } else { format!("{root}/") };
    (2..segments.len()).rev().find_map(|end| {
        let module = segments[1..end].join("/");
        [format!("{prefix}src/{module}.rs"), format!("{prefix}src/{module}/mod.rs"), format!("{prefix}tests/{module}.rs"), format!("{prefix}tests/{module}/main.rs")].into_iter().find(|path| tree.has(path))
    })
}

impl Reach {
    pub fn load(pairs: &Value, roots: &Value, tree: &Tree) -> Reach {
        let text = |cell: &Value| cell.get(1).and_then(Value::as_str).map(str::to_owned);
        let roots: BTreeMap<String, String> = roots["rows"].as_array().into_iter().flatten().filter_map(|row| Some((text(&row[0])?.strip_prefix("crate:")?.to_owned(), text(&row[1])?))).collect();
        let mut tests_of: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for row in pairs["rows"].as_array().into_iter().flatten() {
            let (Some(test), Some(file)) = (text(&row[0]), text(&row[1])) else { continue };
            if let Some(test_file) = module_file(&roots, &test, tree).filter(|test_file| *test_file != file) {
                tests_of.entry(file).or_default().insert(test_file);
            }
        }
        Reach { tests_of }
    }
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || "`'\"()[]{},;<>|*".contains(c)).map(|word| word.trim_end_matches(['.', ':']).trim_start_matches("./")).filter(|word| !word.is_empty()).collect()
}

fn named_paths(text: &str, tree: &Tree) -> Vec<String> {
    let mut by_name: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for path in tree.sizes.keys() {
        by_name.entry(path.rsplit('/').next().unwrap_or(path)).or_default().push(path);
    }
    let mut found: Vec<String> = Vec::new();
    for token in tokens(text) {
        let bare = token.split(':').next().unwrap_or(token);
        let hit = if tree.has(bare) {
            Some(bare.to_owned())
        } else if bare.contains('.') {
            let name = bare.rsplit('/').next().unwrap_or(bare);
            let matching: Vec<&&String> = by_name.get(name).into_iter().flatten().filter(|path| path.ends_with(bare)).collect();
            (matching.len() == 1).then(|| (*matching[0]).clone())
        } else {
            None
        };
        if let Some(path) = hit.filter(|path| !found.contains(path)) {
            found.push(path);
        }
    }
    found
}

fn code_shaped(name: &str) -> bool {
    let starts = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let marked = name.contains('_') || name.chars().skip(1).any(|c| c.is_ascii_uppercase());
    name.len() >= 4 && starts && marked
}

fn symbols(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let parts = tokens(text).into_iter().flat_map(|token| token.split("::"));
    for name in parts.map(|part| part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>()) {
        if code_shaped(&name) && !found.contains(&name) {
            found.push(name);
        }
    }
    found.truncate(SYMBOLS_ASKED);
    found
}

fn defining(repo: &Path, base: &str, names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut args: Vec<String> = ["grep", "-n", "-E", "-I"].map(str::to_owned).to_vec();
    for name in names {
        args.push("-e".to_owned());
        args.push(format!("{DEFINITION}{name}\\b"));
    }
    args.extend([base.to_owned(), "--".to_owned(), "*.rs".to_owned()]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let listed = git(repo, &refs, None).map(|ran| ran.text()).unwrap_or_default();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in listed.lines() {
        if let Some(path) = line.strip_prefix(&format!("{base}:")).and_then(|rest| rest.split(':').next()) {
            *counts.entry(path.to_owned()).or_default() += 1;
        }
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().map(|(path, _)| path).collect()
}

fn cochanged(repo: &Path, base: &str, seeds: &[String], tree: &Tree) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for seed in seeds.iter().take(4) {
        let log = git(repo, &["log", "-n", &COMMITS_READ.to_string(), "--format=%x1e", "--name-only", base, "--", seed], None).map(|ran| ran.text()).unwrap_or_default();
        for commit in log.split('\x1e') {
            let files: Vec<&str> = commit.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
            if files.len() > WIDE_COMMIT {
                continue;
            }
            for file in files.into_iter().filter(|file| !seeds.iter().any(|seed| seed == file) && tree.has(file)) {
                *counts.entry(file.to_owned()).or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().map(|(path, _)| path).collect()
}

pub fn ground(text: &str, repo: &Path, base: &str, tree: &Tree, reach: &Reach) -> Grounded {
    let named = named_paths(text, tree);
    let defining: Vec<String> = defining(repo, base, &symbols(text)).into_iter().filter(|path| !named.contains(path)).collect();
    let seeds: Vec<String> = named.iter().chain(&defining).cloned().collect();
    let cochanged = cochanged(repo, base, &seeds, tree);
    let mut tests: Vec<String> = Vec::new();
    for seed in &seeds {
        for test in reach.tests_of.get(seed).into_iter().flatten() {
            if tree.has(test) && !tests.contains(test) {
                tests.push(test.clone());
            }
        }
    }
    Grounded { named, defining, cochanged, tests }
}
