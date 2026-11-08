use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use north_graph::{Atom, Graph, Term, parse_query};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TestId {
    pub binary: String,
    pub name: String,
}

impl TestId {
    pub fn package(&self) -> &str {
        self.binary.split("::").next().unwrap_or(&self.binary)
    }
}

impl fmt::Display for TestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.binary, self.name)
    }
}

pub struct Region {
    pub node: String,
    pub start: u32,
    pub end: u32,
}

pub struct Facts<'g> {
    graph: &'g Graph,
    tests: BTreeMap<String, TestId>,
    unrecorded: OnceCell<BTreeSet<TestId>>,
    crates: Vec<(String, String)>,
    depends: BTreeMap<String, BTreeSet<String>>,
    pub coverage_commit: Option<String>,
    pub reads_recorded: bool,
}

const RECORDED: &str = "read-set";

fn text(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(Atom::String(value)) => Some(value),
        _ => None,
    }
}

fn var(name: &str) -> Value {
    json!({ "var": name })
}

fn clause(subject: Value, predicate: &str, object: Value) -> Value {
    json!({ "rel": "triple", "args": [subject, ["keyword", predicate], object] })
}

fn crate_name(node: &str) -> Option<&str> {
    node.strip_prefix("crate:")
}

impl<'g> Facts<'g> {
    pub fn read(graph: &'g Graph, repository: &str) -> Result<Self, String> {
        let mut facts = Self { graph, tests: BTreeMap::new(), unrecorded: OnceCell::new(), crates: Vec::new(), depends: BTreeMap::new(), coverage_commit: None, reads_recorded: false };
        for (node, binary) in facts.pairs("in-binary")? {
            let name = node.strip_prefix("test:").and_then(|rest| rest.strip_prefix(binary.as_str())).and_then(|rest| rest.strip_prefix("::"));
            if let Some(name) = name {
                let id = TestId { binary: binary.clone(), name: name.to_owned() };
                facts.tests.insert(node, id);
            }
        }
        for (node, root) in facts.pairs("root")? {
            if let Some(name) = crate_name(&node) {
                let root = root.trim_end_matches('/');
                facts.crates.push((if root == "." { String::new() } else { root.to_owned() }, name.to_owned()));
            }
        }
        for (node, dependency) in facts.pairs("depends-on")? {
            if let (Some(name), Some(dependency)) = (crate_name(&node), crate_name(&dependency)) {
                facts.depends.entry(name.to_owned()).or_default().insert(dependency.to_owned());
            }
        }
        facts.coverage_commit = facts.objects(&format!("coverage:{repository}"), "commit")?.into_iter().next();
        facts.reads_recorded = !facts.objects(&format!("reads:{repository}"), "reads-commit")?.is_empty();
        Ok(facts)
    }

    fn ask(&self, head: &[&str], body: Vec<Value>) -> Result<Vec<Vec<String>>, String> {
        let query = json!({ "find": "r", "rules": [{ "head": { "rel": "r", "args": head.iter().map(|name| var(name)).collect::<Vec<_>>() }, "body": body }], "timeoutMs": 60_000 });
        let query = parse_query(&query.to_string()).map_err(|why| format!("graph query: {why}"))?;
        let answer = self.graph.ask(&query).map_err(|why| format!("graph query: {why}"))?;
        Ok(answer.rows.iter().filter_map(|row| row.iter().map(|term| text(term).map(str::to_owned)).collect()).collect())
    }

    fn pairs(&self, predicate: &str) -> Result<Vec<(String, String)>, String> {
        let rows = self.ask(&["s", "o"], vec![clause(var("s"), predicate, var("o"))])?;
        Ok(rows.into_iter().filter_map(|row| <[String; 2]>::try_from(row).ok()).map(|[subject, object]| (subject, object)).collect())
    }

    fn objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>, String> {
        Ok(self.ask(&["o"], vec![clause(json!(subject), predicate, var("o"))])?.into_iter().flatten().collect())
    }

    fn subjects(&self, predicate: &str, object: &str) -> Result<Vec<String>, String> {
        Ok(self.ask(&["s"], vec![clause(var("s"), predicate, json!(object))])?.into_iter().flatten().collect())
    }

    pub fn test(&self, node: &str) -> TestId {
        if let Some(id) = self.tests.get(node) {
            return id.clone();
        }
        let rest = node.strip_prefix("test:").unwrap_or(node);
        let known = self.tests.values().map(|id| id.binary.as_str()).filter(|binary| rest.strip_prefix(binary).is_some_and(|tail| tail.starts_with("::"))).max_by_key(|binary| binary.len());
        let (binary, name) = match known {
            Some(binary) => (binary, &rest[binary.len() + 2..]),
            None => rest.split_once("::").unwrap_or((rest, "")),
        };
        TestId { binary: binary.to_owned(), name: name.to_owned() }
    }

    pub fn all_tests(&self) -> BTreeSet<TestId> {
        self.tests.values().cloned().collect()
    }

    pub fn unrecorded(&self) -> Result<&BTreeSet<TestId>, String> {
        if let Some(unrecorded) = self.unrecorded.get() {
            return Ok(unrecorded);
        }
        let recorded: BTreeSet<String> = self.ask(&["s"], vec![clause(var("s"), RECORDED, var("o"))])?.into_iter().flatten().collect();
        let unrecorded = self.tests.iter().filter(|(node, _)| !recorded.contains(*node)).map(|(_, id)| id.clone()).collect();
        Ok(self.unrecorded.get_or_init(|| unrecorded))
    }

    pub fn readers(&self, predicate: &str, path: &str) -> Result<BTreeSet<TestId>, String> {
        Ok(self.subjects(predicate, path)?.iter().map(|node| self.test(node)).collect())
    }

    pub fn embedders(&self, path: &str) -> Result<Vec<String>, String> {
        self.subjects("embeds", path)
    }

    pub fn regions(&self, path: &str) -> Result<Vec<Region>, String> {
        let prefix = format!("fn:{path}:");
        let mut regions: Vec<Region> = self
            .subjects("in-file", path)?
            .into_iter()
            .filter_map(|node| {
                let (start, end) = node.strip_prefix(&prefix)?.split_once('-')?;
                let (start, end) = (start.parse().ok()?, end.parse().ok()?);
                Some(Region { node, start, end })
            })
            .collect();
        regions.sort_by_key(|region| (region.start, region.end));
        Ok(regions)
    }

    pub fn reaching(&self, region: &str) -> Result<BTreeSet<TestId>, String> {
        self.readers("reaches", region)
    }

    pub fn reaching_file(&self, path: &str) -> Result<BTreeSet<TestId>, String> {
        let rows = self.ask(&["t"], vec![clause(var("f"), "in-file", json!(path)), clause(var("t"), "reaches", var("f"))])?;
        Ok(rows.into_iter().flatten().map(|node| self.test(&node)).collect())
    }

    pub fn knows_crates(&self) -> bool {
        !self.crates.is_empty()
    }

    pub fn crate_of(&self, path: &str) -> Option<&str> {
        self.crates.iter().filter(|(root, _)| root.is_empty() || path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'))).max_by_key(|(root, _)| root.len()).map(|(_, name)| name.as_str())
    }

    pub fn crate_rooted_at(&self, directory: &str) -> Option<&str> {
        let directory = directory.trim_end_matches('/');
        self.crates.iter().find(|(root, _)| root == directory).map(|(_, name)| name.as_str())
    }

    pub fn manifest_of(&self, path: &str) -> Option<&str> {
        let (directory, file) = path.rsplit_once('/').unwrap_or(("", path));
        (file == "Cargo.toml" || file == "build.rs").then(|| self.crates.iter().find(|(root, _)| root == directory).map(|(_, name)| name.as_str())).flatten()
    }

    pub fn dependencies(&self, name: &str) -> BTreeSet<String> {
        closure(name, |crate_name| self.depends.get(crate_name).into_iter().flatten().cloned().collect())
    }

    pub fn dependents(&self, name: &str) -> BTreeSet<String> {
        closure(name, |crate_name| self.depends.iter().filter(|(_, dependencies)| dependencies.contains(crate_name)).map(|(dependent, _)| dependent.clone()).collect())
    }

    pub fn tests_of(&self, packages: &BTreeSet<String>) -> BTreeSet<TestId> {
        self.tests.values().filter(|id| packages.contains(id.package())).cloned().collect()
    }
}

fn closure(start: &str, next: impl Fn(&str) -> Vec<String>) -> BTreeSet<String> {
    let mut seen = BTreeSet::from([start.to_owned()]);
    let mut queue = vec![start.to_owned()];
    while let Some(name) = queue.pop() {
        for found in next(&name) {
            if seen.insert(found.clone()) {
                queue.push(found);
            }
        }
    }
    seen
}
