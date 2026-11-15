use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::{DocumentMut, Item};

use super::plan::Symbol;
use crate::facts::Facts;

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
    fn referring(&self, symbol: &Symbol, defining: &BTreeSet<String>) -> BTreeSet<String>;
}

pub struct Tree<'f, 'g> {
    root: PathBuf,
    pub commit: String,
    facts: &'f Facts<'g>,
    paths: BTreeSet<String>,
    sets: BTreeSet<String>,
}

impl<'f, 'g> Tree<'f, 'g> {
    pub fn read(root: &Path, base: &str, facts: &'f Facts<'g>) -> Result<Self, String> {
        let commit = git(root, &["rev-parse", "--verify", &format!("{base}^{{commit}}")]).ok_or_else(|| format!("{base} is not a commit of {}", root.display()))?.trim().to_owned();
        match facts.crates_commit.as_deref() {
            Some(indexed) if indexed == commit => {}
            Some(indexed) => return Err(format!("the change graph's crate facts belong to {indexed}, not to the task's base {commit}, so the split cannot read the base's crates")),
            None => return Err("the change graph holds no crate facts, so the split cannot read the base's crates".to_owned()),
        }
        let listed = git(root, &["ls-tree", "-r", "--name-only", &commit]).ok_or("cannot list the base tree")?;
        let paths: BTreeSet<String> = listed.lines().map(str::to_owned).collect();
        let show = |path: &str| git(root, &["show", &format!("{commit}:{path}")]);
        let mut sets = BTreeSet::new();
        if let Some(listed) = show(MERGE_SETS).and_then(|text| text.parse::<DocumentMut>().ok()) {
            sets.extend(listed.get("set").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|set| set.get("path").and_then(Item::as_str)).map(str::to_owned));
        }
        sets.extend(show(ATTRIBUTES).unwrap_or_default().lines().filter(|line| line.split_whitespace().any(|word| word == "merge=set")).filter_map(|line| line.split_whitespace().next()).map(str::to_owned));
        Ok(Self { root: root.to_owned(), commit, facts, paths, sets })
    }

    pub fn map(&self) -> String {
        let mut out = String::from("Crates (name, root: workspace crates it depends on):\n");
        let mut crates: Vec<(&str, &str)> = self.facts.crates().collect();
        crates.sort();
        for (root, name) in crates {
            let depends: Vec<String> = self.facts.depends_on(name).into_iter().collect();
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

    fn module_word(&self, file: &str) -> Option<String> {
        let path = Path::new(file);
        match path.file_stem()?.to_str()? {
            "lib" | "main" => self.crate_of(file).map(|name| name.replace('-', "_")),
            "mod" => path.parent()?.file_name()?.to_str().map(str::to_owned),
            stem => Some(stem.to_owned()),
        }
    }

    fn owned(&self, symbol: &Symbol, files: BTreeSet<String>) -> BTreeSet<String> {
        match &symbol.owner {
            Some(owner) => files.intersection(&self.mentioning(owner)).cloned().collect(),
            None => files,
        }
    }
}

impl Base for Tree<'_, '_> {
    fn tracked(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    fn merged_as_set(&self, path: &str) -> bool {
        self.sets.contains(path)
    }

    fn crate_of(&self, path: &str) -> Option<&str> {
        self.facts.crate_of(path)
    }

    fn manifest_of(&self, name: &str) -> Option<String> {
        self.facts.root_of(name).map(|root| if root.is_empty() { "Cargo.toml".to_owned() } else { format!("{root}/Cargo.toml") })
    }

    fn sees(&self, user: &str, owner: &str) -> bool {
        self.facts.dependencies(user).contains(owner)
    }

    fn defining(&self, symbol: &Symbol) -> BTreeSet<String> {
        let pattern = format!(r"\b{DEFINITION}{0}\b|macro_rules!\s*{0}\b", symbol.name);
        self.owned(symbol, self.grep(&["-E", "-e", &pattern]))
    }

    fn referring(&self, symbol: &Symbol, defining: &BTreeSet<String>) -> BTreeSet<String> {
        let mut reaching = defining.clone();
        for file in defining {
            let module = file.strip_suffix("/mod.rs").or_else(|| file.strip_suffix(".rs")).unwrap_or(file);
            reaching.extend(self.paths.iter().filter(|path| path.strip_prefix(module).is_some_and(|rest| rest.starts_with('/'))).cloned());
            if let Some(word) = self.module_word(file) {
                reaching.extend(self.mentioning(&word));
            }
        }
        self.owned(symbol, self.grep(&["-w", "-F", "-e", &symbol.name])).into_iter().filter(|file| reaching.contains(file)).collect()
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}
