use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::{DocumentMut, Item};

use super::plan::Symbol;
use crate::workspace;

const MERGE_SETS: &str = "harness/merge-sets.toml";
const ATTRIBUTES: &str = ".gitattributes";
const DEFINITION: &str = r"(fn|struct|enum|trait|type|const|static|union|mod)\s+";

pub trait Base {
    fn tracked(&self, path: &str) -> bool;
    fn merged_as_set(&self, path: &str) -> bool;
    fn crate_of(&self, path: &str) -> Option<&str>;
    fn manifest_of(&self, name: &str) -> Option<String>;
    fn sees(&self, user: &str, owner: &str) -> bool;
    fn defining(&self, symbol: &Symbol) -> BTreeSet<String>;
    fn referring(&self, symbol: &Symbol) -> BTreeSet<String>;
}

pub struct Tree {
    root: PathBuf,
    pub commit: String,
    paths: BTreeSet<String>,
    sets: BTreeSet<String>,
    crates: Vec<(String, String)>,
    depends: BTreeMap<String, BTreeSet<String>>,
}

impl Tree {
    pub fn read(root: &Path, base: &str) -> Result<Self, String> {
        let commit = git(root, &["rev-parse", "--verify", &format!("{base}^{{commit}}")]).ok_or_else(|| format!("{base} is not a commit of {}", root.display()))?.trim().to_owned();
        let listed = git(root, &["ls-tree", "-r", "--name-only", &commit]).ok_or("cannot list the base tree")?;
        let paths: BTreeSet<String> = listed.lines().map(str::to_owned).collect();
        let show = |path: &str| git(root, &["show", &format!("{commit}:{path}")]);
        let manifest: DocumentMut = show("Cargo.toml").ok_or("the graph indexes Cargo workspaces, and the base has no Cargo.toml")?.parse().map_err(|why| format!("Cargo.toml: {why}"))?;
        let mut crates = Vec::new();
        let name_of = |manifest: &DocumentMut| manifest.get("package").and_then(|package| package.get("name")).and_then(Item::as_str).map(str::to_owned);
        if let Some(name) = name_of(&manifest) {
            crates.push((String::new(), name));
        }
        let members = manifest.get("workspace").and_then(|workspace| workspace.get("members")).and_then(Item::as_array).into_iter().flatten().filter_map(|member| member.as_str());
        for dir in members.map(|dir| dir.trim_end_matches('/')) {
            let member: Option<DocumentMut> = show(&format!("{dir}/Cargo.toml")).and_then(|text| text.parse().ok());
            if let Some(name) = member.as_ref().and_then(name_of) {
                crates.push((dir.to_owned(), name));
            }
        }
        let local: BTreeSet<&str> = crates.iter().map(|(_, name)| name.as_str()).collect();
        let mut depends: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for package in show("Cargo.lock").map(|lock| workspace::packages(&lock)).transpose()?.unwrap_or_default().iter().filter(|package| !package.source && local.contains(package.name.as_str())) {
            depends.entry(package.name.clone()).or_default().extend(package.dependencies.iter().filter(|name| local.contains(name.as_str())).cloned());
        }
        let mut sets = BTreeSet::new();
        if let Some(listed) = show(MERGE_SETS).and_then(|text| text.parse::<DocumentMut>().ok()) {
            sets.extend(listed.get("set").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|set| set.get("path").and_then(Item::as_str)).map(str::to_owned));
        }
        sets.extend(show(ATTRIBUTES).unwrap_or_default().lines().filter(|line| line.split_whitespace().any(|word| word == "merge=set")).filter_map(|line| line.split_whitespace().next()).map(str::to_owned));
        Ok(Self { root: root.to_owned(), commit, paths, sets, crates, depends })
    }

    pub fn map(&self) -> String {
        let mut out = String::from("Crates (name, root: workspace crates it depends on):\n");
        for (root, name) in &self.crates {
            let depends: Vec<&str> = self.depends.get(name).into_iter().flatten().map(String::as_str).collect();
            out.push_str(&format!("{name}, {}: {}\n", if root.is_empty() { "." } else { root }, depends.join(" ")));
        }
        out.push_str("\nFiles by directory:\n");
        let mut by_directory: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for path in &self.paths {
            let (directory, file) = path.rsplit_once('/').unwrap_or((".", path));
            by_directory.entry(directory).or_default().push(file);
        }
        for (directory, files) in by_directory {
            out.push_str(&format!("{directory}/: {}\n", files.join(" ")));
        }
        out
    }

    fn grep(&self, arguments: &[&str]) -> BTreeSet<String> {
        let prefix = format!("{}:", self.commit);
        let mut command = vec!["grep", "-l"];
        command.extend(arguments);
        command.extend([self.commit.as_str(), "--", "*.rs"]);
        git(&self.root, &command).unwrap_or_default().lines().filter_map(|line| line.strip_prefix(&prefix)).map(str::to_owned).collect()
    }

    fn mentioning(&self, owner: &str) -> BTreeSet<String> {
        let named = |path: &String| Path::new(path).file_stem().is_some_and(|stem| stem == owner);
        let mut files = self.grep(&["-w", "-F", "-e", owner]);
        files.extend(self.paths.iter().filter(|path| named(path)).cloned());
        files
    }

    fn owned(&self, symbol: &Symbol, files: BTreeSet<String>) -> BTreeSet<String> {
        match &symbol.owner {
            Some(owner) => files.intersection(&self.mentioning(owner)).cloned().collect(),
            None => files,
        }
    }
}

impl Base for Tree {
    fn tracked(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    fn merged_as_set(&self, path: &str) -> bool {
        self.sets.contains(path)
    }

    fn crate_of(&self, path: &str) -> Option<&str> {
        self.crates.iter().filter(|(root, _)| root.is_empty() || path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'))).max_by_key(|(root, _)| root.len()).map(|(_, name)| name.as_str())
    }

    fn manifest_of(&self, name: &str) -> Option<String> {
        self.crates.iter().find(|(_, held)| held == name).map(|(root, _)| if root.is_empty() { "Cargo.toml".to_owned() } else { format!("{root}/Cargo.toml") })
    }

    fn sees(&self, user: &str, owner: &str) -> bool {
        let mut seen = BTreeSet::from([user]);
        let mut queue = vec![user];
        while let Some(name) = queue.pop() {
            if name == owner {
                return true;
            }
            queue.extend(self.depends.get(name).into_iter().flatten().map(String::as_str).filter(|next| seen.insert(next)));
        }
        false
    }

    fn defining(&self, symbol: &Symbol) -> BTreeSet<String> {
        let pattern = format!(r"\b{DEFINITION}{0}\b|macro_rules!\s*{0}\b", symbol.name);
        self.owned(symbol, self.grep(&["-E", "-e", &pattern]))
    }

    fn referring(&self, symbol: &Symbol) -> BTreeSet<String> {
        self.owned(symbol, self.grep(&["-w", "-F", "-e", &symbol.name]))
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}
