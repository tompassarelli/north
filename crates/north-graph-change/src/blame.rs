use std::collections::{BTreeMap, BTreeSet};

use north_base::foundation::check_log::{self, log_line, log_text};

use crate::cone::{Cone, untraced_input};
use crate::facts::Facts;
use crate::workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Test { binary: Option<String>, name: String },
    Compile { file: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: Kind,
    pub output: String,
}

impl Failure {
    pub fn test(text: &str) -> Self {
        let kind = match text.trim().split_once(' ') {
            Some((binary, name)) => Kind::Test { binary: Some(binary.to_owned()), name: name.trim().to_owned() },
            None => Kind::Test { binary: None, name: text.trim().to_owned() },
        };
        Self { kind, output: String::new() }
    }

    pub fn compile(file: &str) -> Self {
        Self { kind: Kind::Compile { file: file.to_owned() }, output: String::new() }
    }

    pub fn label(&self) -> String {
        match &self.kind {
            Kind::Test { binary: Some(binary), name } => format!("{binary} {name}"),
            Kind::Test { binary: None, name } => name.clone(),
            Kind::Compile { file } => file.clone(),
        }
    }
}

pub struct Suspect<'a> {
    pub name: &'a str,
    pub cone: &'a Cone,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Blame {
    pub lanes: Vec<String>,
    pub by_failure: BTreeMap<String, Vec<String>>,
    pub untraced: Vec<String>,
}

pub fn of(failures: &[Failure], suspects: &[Suspect], facts: &Facts) -> Blame {
    let mut blame = Blame::default();
    let mut explained = vec![0usize; suspects.len()];
    for failure in failures {
        let holders: Vec<usize> = (0..suspects.len()).filter(|index| holds(failure, suspects[*index].cone, facts)).collect();
        let named: Vec<usize> = holders.iter().copied().filter(|index| names_its_change(failure, suspects[*index].cone)).collect();
        let kept = if named.is_empty() { holders } else { named };
        if kept.is_empty() {
            blame.untraced.push(failure.label());
        }
        for index in &kept {
            explained[*index] += 1;
        }
        blame.by_failure.entry(failure.label()).or_default().extend(kept.iter().map(|index| suspects[*index].name.to_owned()));
    }
    let mut order: Vec<usize> = (0..suspects.len()).filter(|index| explained[*index] > 0).collect();
    order.sort_by_key(|index| std::cmp::Reverse(explained[*index]));
    blame.lanes = order.into_iter().map(|index| suspects[index].name.to_owned()).collect();
    blame
}

fn holds(failure: &Failure, cone: &Cone, facts: &Facts) -> bool {
    match &failure.kind {
        Kind::Test { binary, name } => cone.holds(binary.as_deref(), name),
        Kind::Compile { file } => {
            let crates: BTreeSet<String> = facts.crate_of(file).map(|name| facts.dependencies(name)).unwrap_or_default();
            cone.untraced.is_some() || cone.files.iter().any(|path| path == file || untraced_input(path) || workspace::input(path) || facts.crate_of(path).is_some_and(|name| crates.contains(name)))
        }
    }
}

fn names_its_change(failure: &Failure, cone: &Cone) -> bool {
    let own = match &failure.kind {
        Kind::Compile { file } => Some(file.as_str()),
        Kind::Test { .. } => None,
    };
    cone.files.iter().any(|path| Some(path.as_str()) != own && failure.output.contains(path.as_str()))
}

fn boundary(line: &str) -> bool {
    const STATUS: [&str; 12] = ["PASS", "FAIL", "SLOW", "SIGSEGV", "SIGABRT", "SIGKILL", "TIMEOUT", "LEAK", "START", "TRY", "RETRY", "Summary"];
    let status = line.split_once(" [").is_some_and(|(word, _)| STATUS.contains(&word));
    status || line.starts_with("Cancelling") || line.starts_with("---- ") && line.ends_with(" ----") || line.contains("STDOUT:") || line.contains("STDERR:")
}

pub fn from_log(log: &str, root: &str) -> Vec<Failure> {
    let lines: Vec<&str> = log.lines().map(log_text).collect();
    let raw: Vec<&str> = log.lines().map(log_line).collect();
    let mut failures: Vec<Failure> = check_log::failures(log)
        .into_iter()
        .filter_map(|failure| match failure {
            check_log::Failure::Cargo(name) => Some(name),
            _ => None,
        })
        .map(|name| {
            let output = blocks(&raw, &name);
            Failure { kind: Kind::Test { binary: None, name }, output }
        })
        .collect();
    let mut compiled: BTreeMap<String, String> = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].starts_with("error") {
            index += 1;
            continue;
        }
        let end = (index + 1..lines.len()).find(|next| lines[*next].is_empty() || lines[*next].starts_with("error") || lines[*next].starts_with("warning")).unwrap_or(lines.len());
        let block = lines[index..end].join("\n");
        if let Some(file) = lines[index..end].iter().find_map(|line| line.strip_prefix("--> ")).and_then(|location| location.split(':').next()) {
            let file = file.strip_prefix(root).map_or(file, |rest| rest.trim_start_matches('/'));
            let output = compiled.entry(file.to_owned()).or_default();
            output.push_str(&block);
            output.push('\n');
        }
        index = end;
    }
    failures.extend(compiled.into_iter().map(|(file, output)| Failure { kind: Kind::Compile { file }, output }));
    failures
}

enum Block {
    Outside,
    Opened,
    Body(usize),
}

fn without_panic_location(line: &str) -> String {
    line.split_once("panicked at ").map_or_else(|| line.to_owned(), |(before, after)| format!("{before}{}", after.split_once(' ').map_or("", |(_, rest)| rest)))
}

fn blocks(raw: &[&str], name: &str) -> String {
    let mut output = String::new();
    let mut block = Block::Outside;
    for raw_line in raw {
        let line = raw_line.trim();
        let indent = raw_line.len() - raw_line.trim_start().len();
        let cargo_end = indent == 0 && (line == "failures:" || line.starts_with("test result:"));
        if boundary(line) || cargo_end {
            block = if !cargo_end && line.split_whitespace().any(|word| word == name) { Block::Opened } else { Block::Outside };
            continue;
        }
        if line.is_empty() {
            continue;
        }
        block = match block {
            Block::Outside => continue,
            Block::Opened => Block::Body(indent),
            Block::Body(depth) if indent < depth => Block::Outside,
            body @ Block::Body(_) => body,
        };
        if matches!(block, Block::Body(_)) {
            output.push_str(&without_panic_location(line));
            output.push('\n');
        }
    }
    output
}
